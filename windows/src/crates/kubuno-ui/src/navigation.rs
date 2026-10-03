//! Kubuno primitives — navigation: [`Toolbar`], [`Sidebar`], [`Breadcrumb`],
//! [`Tabs`] and [`StatusBar`].
//!
//! Four of these five have a **predecessor that ships today**
//! (`drive_app_controls::{toolbar, sidebar, breadcrumb_bar}` and, for the tab
//! strip, the Drive info-pane's selector), so this file is written under one
//! constraint above all others: *nobody may be able to tell the difference*.
//! Concretely that means three things, and they are worth stating because they
//! are what makes this module short:
//!
//! * **The item model is the replica's.** A tool bar item is a
//!   [`StripItem`] — a `ToolStripButton`, `ToolStripLabel`, `ToolStripSeparator`
//!   or `ToolStripMenuItem` — so `text`, `image`, `enabled`, `visible`,
//!   `checked`, `alignment` and `overflow` are the .NET fields, in one storage
//!   location, and nothing here restates them. A sidebar row and a breadcrumb
//!   segment are the same items on a **vertical** and a horizontal strip.
//! * **The arrangement is the predecessor's.** [`toolbar::arrange`] already
//!   implements « from last to first; if too narrow, draw the ellipsis
//!   button », and [`layout_breadcrumbs`] already folds the head of a path
//!   behind a `…`. Those two are pure functions with their own tests; this
//!   module calls them. The sidebar's vertical stacking is
//!   [`layout_items`] — the replica's own strip engine — and the tab strip's
//!   wrapping is [`tab_strip`] / [`tab_rows`] / [`tab_rects`], likewise.
//! * **Only the paint is new**, and every number in it is either a token from
//!   [`crate::metrics`] or a constant published by the predecessor. The
//!   handful that are neither are declared at the top of this file with the
//!   shipping call site they were read from.
//!
//! ## The two deliberate departures from the predecessors
//!
//! 1. **Chevrons are geometry, not text.** The predecessors draw `` and ``
//!    as Segoe Fluent codepoints through `formats().icon_small`; the brief for
//!    this crate (rule 6) requires [`Canvas::vector_icon`] with a name from
//!    `assets/lucide-icons.txt`. The rectangle, the size and the colour are
//!    unchanged — only the outline is now the design system's own.
//! 2. **The « New » button's plus is a `Plus` icon.** The predecessor builds it
//!    from two rounded bars because « `Canvas` exposes no path API »; it does
//!    expose `vector_icon`, and lucide's `Plus` *is* two rounded bars on a 24
//!    grid. Same shape, one less hand-rolled primitive.
//!
//! Everything else — every fill, every offset, every token — is the
//! predecessor's, and the unit tests at the bottom pin the geometry against it.

use std::borrow::Cow;
use std::ops::{Deref, DerefMut};

use drive_app_controls::breadcrumb_bar::{
    layout_breadcrumbs, BreadcrumbLayout, BreadcrumbLayoutParams,
};
use drive_app_controls::sidebar::{
    sidebar_pane_width, SidebarMode, ROW_ICON_SIZE, ROW_ICON_TEXT_GAP, ROW_TEXT_RIGHT_MARGIN,
    SIDEBAR_ROW_HEIGHT, SIDEBAR_SECTION_GAP,
};
use drive_app_controls::themed_icon::icon_name;
use drive_app_controls::toolbar::{
    arrange, ItemMeasure, OverflowBehavior, ToolbarArrangement, BUTTON_PADDING, ICON_SIZE,
    ITEM_SPACING,
};
use drive_app_controls::{Canvas, Rect};
use kubuno_controls::enums::{Padding, Size};
use kubuno_controls::layout_panels::{
    tab_display_rect, tab_rects, tab_rows, tab_strip, tab_strip_thickness, TabAlignment,
    TabControl, TabPage, TabSizeMode,
};
use kubuno_controls::toolstrip::{
    layout_items, StatusStrip, StripAxis, StripItem, StripItemInput, StripLayout, ToolStrip,
    ToolStripButton, ToolStripGripStyle, ToolStripItemDisplayStyle, ToolStripItemOverflow,
    ToolStripLabel, ToolStripLayoutStyle, ToolStripMenuItem, ToolStripSeparator,
    ToolStripStatusLabel,
};
use kubuno_controls::host::{self, vk, Modifiers};
use kubuno_controls::Control;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::lists::{self, Menu, MenuEntry};
use crate::metrics::{control, height, pill, radius, space};
use crate::{Widget, WidgetState};

// ─────────────────────────────────────────────────────────────────────────────
// The numbers this file owns
//
// A number lands here only when neither `crate::metrics` nor a predecessor's
// public constant answers for it. Each one names the shipping call site it was
// read from, so it can be checked rather than believed.
// ─────────────────────────────────────────────────────────────────────────────

/// Opacity of a disabled command. `disabled:opacity-40` in the web, and the
/// private `DISABLED_OPACITY` of `drive_app_controls::toolbar` — a disabled
/// button keeps its hue and only fades, it is never recoloured.
const DISABLED_OPACITY: f32 = 0.40;

/// A tool bar separator's hit box: « a 1 DIP vertical line, 6 DIP margin on
/// either side — its hit zone covers the 12 DIP » (`drive-app/src/ui/layout.rs`,
/// `Hot::CmdSeparator`).
const SEPARATOR_WIDTH: f32 = 12.0;
/// How far the separator's line is inset from the row's top and bottom
/// (`drive_app_controls::toolbar::draw`, `ButtonVisual::Separator`).
const SEPARATOR_INSET: f32 = 6.0;

/// An icon-only command's box, and a command that also opens a menu.
/// `drive-app`'s command bar lays the left block out at 40 and the right
/// block's drop-downs at 52; 40 is also `ToolbarSize::Medium`'s button width.
const ICON_BUTTON_WIDTH: f32 = 40.0;
const DROPDOWN_WIDTH: f32 = 52.0;

/// The label column of a command that shows an icon **and** a caption: the icon
/// sits at `left + 8 … left + 30` and the caption starts at `left + 36`
/// (`toolbar::draw`, `ButtonVisual::IconLabel`), and the shipping bar sizes such
/// a button as `36 + text + 16` (`layout.rs`, the Recycle Bin context).
const LABEL_TEXT_LEFT: f32 = 36.0;
const LABEL_TEXT_RIGHT_PAD: f32 = 16.0;
/// The icon's own box inside a labelled command, from the row's left edge.
const LABEL_ICON_LEFT: f32 = 8.0;
const LABEL_ICON_RIGHT: f32 = 30.0;
/// The chevron column of a labelled drop-down (`toolbar::draw`,
/// `ButtonVisual::NewButton`: text stops at `right - 22`, chevron at
/// `right - 22 … right - 6`).
const MENU_CHEVRON_COLUMN: f32 = 22.0;
const MENU_CHEVRON_INSET: f32 = 6.0;
/// The chevron column of an icon-only drop-down (`ButtonVisual::IconChevron`:
/// the icon stops at `right - 16`, the chevron sits at `right - 18 … right - 2`).
const ICON_CHEVRON_GAP: f32 = 16.0;
const ICON_CHEVRON_LEFT: f32 = 18.0;
const ICON_CHEVRON_RIGHT: f32 = 2.0;
/// A chevron glyph's box, matching `formats().icon_small`'s 12 DIP em.
const CHEVRON_SIZE: f32 = 12.0;

/// The sidebar's chevron column, taken off the row's indent
/// (`sidebar_view::draw`: `left + indent - 24 … left + indent - 4`).
const SIDEBAR_CHEVRON_LEFT: f32 = 24.0;
const SIDEBAR_CHEVRON_RIGHT: f32 = 4.0;
/// The icon a sidebar row actually draws inside its 20 DIP box
/// (`sidebar_view::draw` passes 16 to every `RowIcon` arm).
const SIDEBAR_ICON_GLYPH: f32 = 16.0;

/// A breadcrumb segment's own padding, both sides together —
/// `BreadcrumbBarItemPadding = 8,0` (`drive-app/src/ui/layout.rs`, `SEG_PAD`).
const SEGMENT_PADDING: f32 = 16.0;
/// Where a segment's label starts inside its box (`breadcrumb_bar::draw`).
const SEGMENT_TEXT_LEFT: f32 = 8.0;
/// The block between two segments: margin 2 + padding 4 + glyph 12 + padding 4
/// (`BreadcrumbLayoutParams::chevron_block`, as `drive-app` fills it in).
const CHEVRON_BLOCK: f32 = 22.0;
/// Where the chevron's glyph sits inside that block (`breadcrumb_bar::draw`).
const CHEVRON_OFFSET: f32 = 6.0;
/// The `…` button on overflow (`BreadcrumbLayoutParams::ellipsis_width`).
const ELLIPSIS_WIDTH: f32 = 24.0;

/// The status bar's own text inset — `StatusBar.xaml`'s `Padding="8,0,0,0"`,
/// which is `space::SM`. Kept named so the paint body reads as the XAML does.
const STATUS_TEXT_INSET: f32 = space::SM;
/// The git widget's icon column and label start (`drive-app`'s
/// `draw_status_git`: icon at `left + 10 … left + 26`, label at `left + 32`).
const STATUS_ICON_LEFT: f32 = 10.0;
const STATUS_ICON_RIGHT: f32 = 26.0;
const STATUS_LABEL_LEFT: f32 = 32.0;
/// How far the hover pill of a status-bar button is deflated
/// (`draw_status_git`: `rect.inflate(-2.0, -3.0)`).
const STATUS_HOVER_INSET: (f32, f32) = (2.0, 3.0);

/// Whether a tab strip runs down a side. `TabAlignment::is_vertical` says this
/// in the replica but is private to it, and the two members it covers are the
/// enum's own definition, not a number this file invents.
fn strip_is_vertical(alignment: TabAlignment) -> bool {
    matches!(alignment, TabAlignment::Left | TabAlignment::Right)
}

/// A colour at reduced opacity — how a disabled command fades.
fn dimmed(color: &D2D1_COLOR_F) -> D2D1_COLOR_F {
    D2D1_COLOR_F { a: color.a * DISABLED_OPACITY, ..*color }
}

/// `rect` deflated by `pad`. The replica's own `inset_margin` is private, and
/// this is the whole of it.
fn inset(rect: Rect, pad: Padding) -> Rect {
    Rect::new(
        rect.left + pad.left,
        rect.top + pad.top,
        rect.right - pad.right,
        rect.bottom - pad.bottom,
    )
}

/// The lucide chevron for an expanded / collapsed state, and the one a trail
/// draws between two segments. Names are from `assets/lucide-icons.txt`.
const CHEVRON_DOWN: &str = "ChevronDown";
const CHEVRON_RIGHT: &str = "ChevronRight";

// The « More » / `…` buttons draw lucide's `MoreHorizontal` (« ellipsis »)
// at 16 DIP — `<MoreHorizontal size={16} />` in `Breadcrumb.tsx` — rather than
// a typographic ellipsis, whose weight and baseline depend on the face.

/// A tab's own vertical padding — `pt-2` for `md` and `pt-1.5` for `sm`
/// (`Tabs.tsx`). The horizontal one is `px-4` / `px-3`, i.e. `space::LG` and
/// `space::MD`.
const TAB_PAD_Y_MD: f32 = space::SM;
const TAB_PAD_Y_SM: f32 = 6.0;
/// The line a tab label sits on: `text-sm` is 14/20 in Tailwind's scale, so the
/// row is 20 DIP tall whatever the caption says.
const TAB_TEXT_LINE: f32 = 20.0;
/// The `md` underline tab is a HEIGHT, not a pair of paddings: `px-4 h-12`
/// (`Tabs.tsx`: « the height of a tab is a decision about the strip »).
const TAB_HEIGHT_MD: f32 = 48.0;
/// `gap-1` between two tabs of the underline grid.
const TAB_GAP: f32 = 4.0;
/// A scroll arrow of an overflowing strip: `px-0.5` around a 16 DIP
/// `ChevronLeft` / `ChevronRight` (`Tabs.tsx`, `arrowCls`).
const TAB_ARROW_PAD: f32 = 2.0;
const TAB_ARROW_ICON: f32 = 16.0;
const TAB_ARROW_WIDTH: f32 = TAB_ARROW_ICON + 2.0 * TAB_ARROW_PAD;
/// `scrollByPage`: `Math.max(120, clientWidth * 0.75)`.
const TAB_PAGE_MIN: f32 = 120.0;
const TAB_PAGE_FRACTION: f32 = 0.75;
/// The « 1px slack » `syncArrows` allows so fractional widths never leave a
/// phantom arrow.
const TAB_SCROLL_SLACK: f32 = 1.0;

/// `maxSegmentWidth = '14rem'` (`Breadcrumb.tsx`): the longest a single
/// segment may grow before it truncates with an ellipsis.
const SEGMENT_MAX_TEXT: f32 = 224.0;
/// `MoreHorizontal size={16}` on the collapsed trail's `…` button.
const MORE_ICON: f32 = 16.0;

/// The keyboard focus ring: `focus-visible:ring-2 ring-primary`, drawn inward
/// like every other ring of the crate (`buttons::glyph_metrics::FOCUS_RING`).
const FOCUS_RING: f32 = 2.0;

/// Room added to a measured caption so DirectWrite never trims it: the
/// measurement is fractional and the layout box is snapped to physical pixels,
/// which could shave the last fraction off and trigger the ellipsis on a label
/// that fits (« 12 élémen… » in a bar with room to spare).
const TEXT_SLACK: f32 = 1.0;

/// A caption's width as a box must be sized to hold it — the ceiled extent
/// plus [`TEXT_SLACK`]. Empty text takes no room at all.
fn text_width(c: &dyn Canvas, text: &str, format: &windows::Win32::Graphics::DirectWrite::IDWriteTextFormat) -> f32 {
    if text.is_empty() {
        0.0
    } else {
        c.measure(text, format).ceil() + TEXT_SLACK
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Shared interaction vocabulary
//
// Every strip of this family is navigated the same way by the keyboard (the
// ARIA « roving tabindex » patterns: toolbar, tablist, tree) and paints the
// same four transient states. These are the pure parts, so each primitive's
// behaviour is unit-testable without a window.
// ─────────────────────────────────────────────────────────────────────────────

/// One step of roving keyboard navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    /// Left (horizontal strip) / Up (vertical strip).
    Prev,
    /// Right / Down.
    Next,
    /// Home.
    First,
    /// End.
    Last,
}

/// Where roving focus lands after `key`, over `n` positions of which only those
/// `usable` accepts can take focus. `wrap` follows the ARIA patterns: a tab
/// list and a tool bar wrap around, a tree does not. `from == None` enters the
/// strip: `Next`/`First` land on the first usable position, `Prev`/`Last` on
/// the last one. `None` means « nowhere usable ».
pub fn roving_step(
    n: usize,
    from: Option<usize>,
    key: NavKey,
    wrap: bool,
    usable: impl Fn(usize) -> bool,
) -> Option<usize> {
    let first = (0..n).find(|&i| usable(i));
    let last = (0..n).rev().find(|&i| usable(i));
    let from = match from {
        Some(i) if i < n => i,
        _ => {
            return match key {
                NavKey::Next | NavKey::First => first,
                NavKey::Prev | NavKey::Last => last,
            }
        }
    };
    match key {
        NavKey::First => first,
        NavKey::Last => last,
        NavKey::Next => (from + 1..n)
            .find(|&i| usable(i))
            .or(if wrap { first } else { Some(from) }),
        NavKey::Prev => (0..from)
            .rev()
            .find(|&i| usable(i))
            .or(if wrap { last } else { Some(from) }),
    }
}

/// The [`NavKey`] a virtual-key code means on a strip laid out `vertical`ly or
/// not. The cross-axis arrows mean nothing to a strip and answer `None`.
pub fn nav_key_of(code: u16, vertical: bool) -> Option<NavKey> {
    let (prev, next) = if vertical { (vk::UP, vk::DOWN) } else { (vk::LEFT, vk::RIGHT) };
    match code {
        c if c == prev => Some(NavKey::Prev),
        c if c == next => Some(NavKey::Next),
        vk::HOME => Some(NavKey::First),
        vk::END => Some(NavKey::Last),
        _ => None,
    }
}

/// Consumes this frame's roving keys (the axis arrows, Home, End — without
/// modifiers) from the host queue and returns them in order. Call it only
/// while the strip holds the focus: a consumed key is invisible to the rest
/// of the page.
pub fn take_nav_keys(vertical: bool) -> Vec<NavKey> {
    let events = host::consume(|e| match e {
        host::InputEvent::Key { vk: code, down: true, mods, .. } => {
            mods.matches(Modifiers::NONE) && nav_key_of(*code, vertical).is_some()
        }
        _ => false,
    });
    events
        .into_iter()
        .filter_map(|e| match e {
            host::InputEvent::Key { vk: code, .. } => nav_key_of(code, vertical),
            _ => None,
        })
        .collect()
}

/// Consumes Enter or Space (no modifiers) — « activate the focused item ».
pub fn take_activate() -> bool {
    let enter = host::take_key(vk::ENTER, Modifiers::NONE);
    let space = host::take_key(vk::SPACE, Modifiers::NONE);
    enter + space > 0
}

/// What the host knows about the pointer and the focus of a strip, handed to a
/// primitive's `paint_with`. A `ToolStripItem` is a `Component` and cannot
/// observe any of this itself; the indices are indices into the primitive's own
/// item collection.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct StripPaint {
    /// The item under the pointer.
    pub hot: Option<usize>,
    /// The item the left button is held on.
    pub pressed: Option<usize>,
    /// The item holding the keyboard focus (roving tab stop).
    pub focused: Option<usize>,
    /// `:focus-visible` — paint the ring around [`StripPaint::focused`] (and
    /// around the overflow button when [`StripPaint::overflow_focused`]).
    pub focus_visible: bool,
    /// The overflow button (tool bar « More », breadcrumb `…`) under the pointer.
    pub hot_overflow: bool,
    /// Its menu is open: the button keeps its hover fill, like a trigger does.
    pub overflow_open: bool,
    /// The overflow button holds the keyboard focus.
    pub overflow_focused: bool,
}

impl StripPaint {
    /// Only a hovered item — what the older `paint_*(…, hot, …)` entry points
    /// could say.
    pub fn hot(hot: Option<usize>) -> Self {
        Self { hot, ..Self::default() }
    }

    fn ring(&self, i: usize) -> bool {
        self.focus_visible && self.focused == Some(i)
    }

    fn ring_overflow(&self) -> bool {
        self.focus_visible && self.overflow_focused
    }
}

/// The keyboard ring around `rect`, inside it (so a strip clipped to its
/// bounds never loses half of it).
fn paint_ring(c: &dyn Canvas, rect: Rect, corner: f32) {
    c.stroke_rounded_w(&rect, corner, &c.theme().accent, FOCUS_RING);
}

// ─────────────────────────────────────────────────────────────────────────────
// Overflow menus — floating surfaces
//
// A tool bar's « More » button and a breadcrumb's `…` open a `MenuDropdown`
// (`Breadcrumb.tsx` renders `<MenuDropdown … pos={{ top: r.bottom + 4, left:
// r.left }}>`). On the desktop that menu is a floating window of its own
// (`kubuno_controls::host::popup`), placed against the MONITOR so it may hang
// past the owner window, exactly like the web menu escapes its container.
// ─────────────────────────────────────────────────────────────────────────────

/// The gap between a trigger and the menu it opens (`r.bottom + 4`).
pub const MENU_GAP: f32 = 4.0;
/// How close a menu may come to the viewport's edge (`MenuDropdown`'s 8 px).
pub const MENU_VIEWPORT_EDGE: f32 = 8.0;
/// The margin a popup window keeps around a menu panel for its shadow
/// (`SHADOW_MENU` spreads ~7 DIP; the gallery's reference menu keeps 10).
pub const MENU_SHADOW_MARGIN: f32 = 10.0;

/// Where a menu of `size` opens under `anchor`, inside `screen` (the monitor
/// work area, in the same coordinates): left-aligned on the trigger, pulled
/// back inside the viewport, and flipped ABOVE the trigger when it would run
/// off the bottom.
pub fn place_menu(anchor: Rect, size: Size, screen: Rect) -> Rect {
    let e = MENU_VIEWPORT_EDGE;
    let left = if anchor.left + size.width > screen.right - e {
        (screen.right - e - size.width).max(screen.left + e)
    } else {
        anchor.left.max(screen.left + e)
    };
    let below = anchor.bottom + MENU_GAP;
    let top = if below + size.height > screen.bottom - e {
        (anchor.top - MENU_GAP - size.height).max(screen.top + e)
    } else {
        below
    };
    Rect::new(left, top, left + size.width, top + size.height)
}

/// The popup window a menu panel needs: the panel plus its shadow margin.
pub fn menu_popup_bounds(panel: Rect) -> Rect {
    panel.inflate(MENU_SHADOW_MARGIN, MENU_SHADOW_MARGIN)
}

/// Shows `menu` at `panel` (client DIP) in an interactive popup window of its
/// own. Hover and clicks over it come back through `Frame::mouse` in client
/// coordinates, so the caller hit-tests with [`Menu::item_at`] against the
/// same `panel`. Call it every frame the menu is open.
pub fn show_menu_popup(menu: Menu, panel: Rect) {
    let pb = menu_popup_bounds(panel);
    let local = Rect::new(
        panel.left - pb.left,
        panel.top - pb.top,
        panel.right - pb.left,
        panel.bottom - pb.top,
    );
    host::popup(pb, move |canvas| menu.paint(canvas, local, WidgetState::REST));
}

/// The label an item reads as in an overflow menu: its caption, else its
/// tooltip (an icon-only command's `title`), else its icon key.
fn menu_label(item: &StripItem) -> String {
    let it = item.item();
    if !it.text.is_empty() {
        it.text.clone()
    } else if !it.tool_tip_text.is_empty() {
        it.tool_tip_text.clone()
    } else {
        it.image.clone().unwrap_or_default()
    }
}

/// One strip item as a menu row: separators stay separators, labels become
/// section headers, commands become entries carrying the icon, the enabled
/// flag and — for a toggle — the tick.
fn menu_row_of(item: &StripItem) -> StripItem {
    match item {
        StripItem::Separator(_) => lists::separator(),
        StripItem::Label(_) | StripItem::StatusLabel(_) => lists::section(menu_label(item)),
        _ => {
            let it = item.item();
            let mut e = MenuEntry::new(menu_label(item)).enabled(it.enabled);
            if let Some(name) = item_icon(item) {
                e = e.icon(name);
            }
            if let StripItem::Button(b) = item {
                if b.check_on_click {
                    e = e.checked(b.checked);
                }
            }
            e.build()
        }
    }
}

/// Sets an item's tooltip — the web's `title`, which an icon-only command
/// needs to be named at all (in its tooltip and in the overflow menu).
pub fn with_tooltip(mut item: StripItem, tip: &str) -> StripItem {
    item_block_mut(&mut item).tool_tip_text = tip.to_string();
    item
}

/// The mutable twin of `StripItem::item`, which the replica does not expose.
fn item_block_mut(item: &mut StripItem) -> &mut kubuno_controls::toolstrip::ToolStripItem {
    match item {
        StripItem::Button(b) => &mut b.item,
        StripItem::Label(l) => &mut l.item,
        StripItem::StatusLabel(s) => &mut s.item,
        StripItem::Separator(s) => &mut s.item,
        StripItem::MenuItem(m) => &mut m.base.item,
        StripItem::ComboBox(c) => &mut c.host.item,
        StripItem::TextBox(t) => &mut t.host.item,
        StripItem::ProgressBar(p) => &mut p.host.item,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Item constructors
//
// The replica's item chain is expressive but verbose to build by hand; these
// name the six shapes the Kubuno navigation surfaces actually use, so a caller
// (and this crate's own gallery) never has to remember which of `DisplayStyle`,
// `CheckOnClick` and `DropDownItems` selects which visual.
// ─────────────────────────────────────────────────────────────────────────────

/// An icon-only command — `ToolStripButton`, `DisplayStyle = Image`.
pub fn icon_item(icon: &str) -> StripItem {
    let mut b = ToolStripButton::default();
    b.item.image = Some(icon.to_string());
    b.item.display_style = ToolStripItemDisplayStyle::Image;
    StripItem::Button(b)
}

/// A command with an icon **and** a caption — `DisplayStyle = ImageAndText`.
pub fn icon_label_item(icon: &str, text: &str) -> StripItem {
    let mut b = ToolStripButton::default();
    b.item.image = Some(icon.to_string());
    b.item.text = text.to_string();
    StripItem::Button(b)
}

/// A toggling command — `CheckOnClick = true`, so `Checked` drives the pill.
pub fn toggle_item(icon: &str, on: bool) -> StripItem {
    let mut b = ToolStripButton::default();
    b.item.image = Some(icon.to_string());
    b.item.display_style = ToolStripItemDisplayStyle::Image;
    b.check_on_click = true;
    b.checked = on;
    StripItem::Button(b)
}

/// A command that opens a menu — `ToolStripMenuItem`. With `text` empty it is
/// an icon plus a chevron; with a caption it is the « New ▾ » shape.
pub fn menu_item(icon: &str, text: &str) -> StripItem {
    let mut m = ToolStripMenuItem::new(text);
    m.base.item.image = Some(icon.to_string());
    if text.is_empty() {
        m.base.item.display_style = ToolStripItemDisplayStyle::Image;
    }
    StripItem::MenuItem(m)
}

/// A divider.
pub fn separator_item() -> StripItem {
    StripItem::Separator(ToolStripSeparator::default())
}

/// Inert text — `ToolStripLabel`.
pub fn label_item(text: &str) -> StripItem {
    StripItem::Label(ToolStripLabel::new(text))
}

/// A sidebar **section header** — inert text with the same icon column and
/// indent an ordinary row has, so the two line up. A `ToolStripLabel`, because
/// a header is not clickable and has no check state to carry.
pub fn section_item(icon: &str, text: &str, indent: f32) -> StripItem {
    let mut l = ToolStripLabel::new(text);
    l.item.image = Some(icon.to_string());
    l.item.padding = Padding::new(indent, 0.0, 0.0, 0.0);
    StripItem::Label(l)
}

/// A sidebar navigation row: an icon, a label, and `Checked` for « this is the
/// current location ». The indent is carried in the item's own `Padding.Left`
/// — the replica field that already means « space before my content » — and a
/// nesting level costs
/// [`INDENT_PER_LEVEL`](drive_app_controls::sidebar::INDENT_PER_LEVEL).
pub fn nav_item(icon: &str, text: &str, active: bool, indent: f32) -> StripItem {
    let mut b = ToolStripButton::default();
    b.item.image = Some(icon.to_string());
    b.item.text = text.to_string();
    b.item.padding = Padding::new(indent, 0.0, 0.0, 0.0);
    b.check_on_click = true;
    b.checked = active;
    StripItem::Button(b)
}

/// A status-bar cell — `ToolStripStatusLabel`, whose `Spring` shares out the
/// leftover width exactly as a `StatusStrip` does.
pub fn status_item(text: &str, spring: bool) -> StripItem {
    let mut l = ToolStripStatusLabel::new(text);
    l.spring = spring;
    StripItem::StatusLabel(l)
}

/// The vector-icon name an item's `Image` key resolves to, if the design
/// system carries that geometry. `None` means « nothing to draw », never a
/// fallback glyph: a missing icon is a missing asset, not a paint bug.
fn item_icon(item: &StripItem) -> Option<&'static str> {
    item.item().image.as_deref().and_then(icon_name)
}

// ═════════════════════════════════════════════════════════════════════════════
// Toolbar
// ═════════════════════════════════════════════════════════════════════════════

/// Which of the command bar's six shapes an item is. Derived from the replica —
/// nothing here is stored twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Visual {
    Separator,
    Label,
    Icon,
    Toggle,
    IconLabel,
    IconChevron,
    MenuLabel,
}

fn visual_of(item: &StripItem) -> Visual {
    match item {
        StripItem::Separator(_) => Visual::Separator,
        StripItem::Label(_) => Visual::Label,
        StripItem::MenuItem(m) => {
            if m.base.item.display_style.shows_text() && !m.base.item.text.is_empty() {
                Visual::MenuLabel
            } else {
                Visual::IconChevron
            }
        }
        StripItem::Button(b) => {
            if b.check_on_click {
                Visual::Toggle
            } else if b.item.display_style.shows_text() && !b.item.text.is_empty() {
                Visual::IconLabel
            } else {
                Visual::Icon
            }
        }
        _ => Visual::Icon,
    }
}

/// The Kubuno command bar.
///
/// Owns a [`ToolStrip`] for the item model — the buttons, the separators, the
/// labels, the drop-downs, `Enabled`, `Checked` and the per-item overflow rule
/// — and arranges them with [`toolbar::arrange`], the shipping predecessor's own
/// pure primitive. The paint is the Kubuno one: flat on the module panel, no
/// card and no border, a full-pill hover, an `accent_light` pill under an active
/// toggle.
///
/// [`toolbar::arrange`]: drive_app_controls::toolbar::arrange
#[derive(Clone)]
pub struct Toolbar {
    inner: ToolStrip,
    /// The row height. `height::BUTTON_MD` is the shipping command bar's
    /// `CMDBAR_BUTTON_HEIGHT`, which is also the web's `Button size="md"`.
    pub row_height: f32,
    /// Whether the bar paints its own `layer_background` band. Off by default:
    /// the web command bar has no surface of its own and sits straight on
    /// whatever holds it (a card body, the module panel), so a band would show
    /// as a white strip across a tinted card. Drive's title-bar command bar,
    /// which does sit on its own layer, turns it on ([`Toolbar::with_band`]).
    pub band: bool,
}

/// A keyboard stop of a tool bar: one of its items, or the « More » button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarTarget {
    Item(usize),
    Overflow,
}

impl Default for Toolbar {
    fn default() -> Self {
        Self::new()
    }
}

impl Toolbar {
    pub fn new() -> Self {
        // A Kubuno command bar is a `ToolStrip` with the three defaults a flat
        // bar needs changed: no move handle, docked to the top of its panel,
        // and 16 DIP images (`ICON_SIZE`).
        let mut inner = ToolStrip::new();
        inner.grip_style = ToolStripGripStyle::Hidden;
        inner.layout_style = ToolStripLayoutStyle::HorizontalStackWithOverflow;
        inner.image_scaling_size = Size::new(ICON_SIZE, ICON_SIZE);
        Self { inner, row_height: height::BUTTON_MD, band: false }
    }

    /// Adds an item, returning `self` so a bar reads as one expression.
    pub fn with(mut self, item: StripItem) -> Self {
        self.inner.items.push(item);
        self
    }

    /// Turns the bar's own `layer_background` band on or off (see
    /// [`Toolbar::band`]).
    pub fn with_band(mut self, on: bool) -> Self {
        self.band = on;
        self
    }

    /// The width one item wants, from its visual and its measured caption.
    fn item_width(&self, c: &dyn Canvas, item: &StripItem) -> f32 {
        let f = c.formats();
        match visual_of(item) {
            Visual::Separator => SEPARATOR_WIDTH,
            Visual::Icon | Visual::Toggle => ICON_BUTTON_WIDTH,
            Visual::IconChevron => DROPDOWN_WIDTH,
            Visual::Label => text_width(c, &item.item().text, &f.body_small) + 2.0 * BUTTON_PADDING,
            Visual::IconLabel => {
                LABEL_TEXT_LEFT
                    + text_width(c, &item.item().text, &f.body_small)
                    + LABEL_TEXT_RIGHT_PAD
            }
            Visual::MenuLabel => {
                LABEL_TEXT_LEFT + text_width(c, &item.item().text, &f.body) + MENU_CHEVRON_COLUMN
            }
        }
    }

    /// Whether item `i` can take the keyboard focus: a visible, enabled
    /// command — never a separator nor inert text (`role="toolbar"` skips
    /// them).
    pub fn is_focusable(&self, i: usize) -> bool {
        self.inner.items.get(i).is_some_and(|it| {
            it.item().visible
                && it.item().enabled
                && !matches!(visual_of(it), Visual::Separator | Visual::Label)
        })
    }

    /// The bar's keyboard stops in visual order — the ARIA toolbar's roving
    /// tab stop walks these with the arrows, Home and End. Pure: it takes an
    /// arrangement already computed.
    pub fn focus_order(&self, a: &ToolbarArrangement) -> Vec<ToolbarTarget> {
        let mut v: Vec<ToolbarTarget> = a
            .visible
            .iter()
            .filter(|(i, _)| self.is_focusable(*i))
            .map(|(i, _)| ToolbarTarget::Item(*i))
            .collect();
        if a.overflow_button.is_some() {
            v.push(ToolbarTarget::Overflow);
        }
        v
    }

    /// The menu the « More » button opens: the overflowed items, in order,
    /// as `MenuDropdown` rows (an icon-only command reads as its tooltip).
    /// Leading, trailing and doubled separators are dropped — a rule is only
    /// ever drawn between two groups.
    pub fn overflow_menu(&self, a: &ToolbarArrangement) -> Menu {
        let mut rows: Vec<StripItem> = Vec::new();
        for &i in &a.overflow {
            let Some(item) = self.inner.items.get(i) else { continue };
            if !item.item().visible {
                continue;
            }
            let sep = matches!(item, StripItem::Separator(_));
            if sep && rows.last().is_none_or(|r| matches!(r, StripItem::Separator(_))) {
                continue;
            }
            rows.push(menu_row_of(item));
        }
        while rows.last().is_some_and(|r| matches!(r, StripItem::Separator(_))) {
            rows.pop();
        }
        Menu::with_items(rows)
    }

    /// Which item a row of [`Toolbar::overflow_menu`] stands for — the inverse
    /// of the row mapping, so a chosen row activates the real command.
    pub fn overflow_item_for_row(&self, a: &ToolbarArrangement, row: usize) -> Option<usize> {
        let mut k = 0usize;
        let mut last_sep = true;
        let mut out = None;
        for &i in &a.overflow {
            let Some(item) = self.inner.items.get(i) else { continue };
            if !item.item().visible {
                continue;
            }
            let sep = matches!(item, StripItem::Separator(_));
            if sep && last_sep {
                continue;
            }
            last_sep = sep;
            if k == row {
                out = (!sep).then_some(i);
                break;
            }
            k += 1;
        }
        out
    }

    /// Every visible item's index and desired width, in collection order.
    pub fn item_widths(&self, c: &dyn Canvas) -> Vec<(usize, f32)> {
        self.inner
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.item().visible)
            .map(|(i, it)| (i, self.item_width(c, it)))
            .collect()
    }

    /// Arranges already-measured items into `bounds`.
    ///
    /// Pure — no canvas — so the overflow behaviour is unit-testable, and it is
    /// [`toolbar::arrange`] that decides it: the same « from last to first »
    /// walk, the same [`OVERFLOW_BUTTON_WIDTH`] reserve, the same per-item rule.
    /// The only thing added is the shift onto `bounds`, which `arrange` reports
    /// from `x = 0`.
    ///
    /// The returned indices are indices into [`ToolStrip::items`], not into
    /// `widths`.
    ///
    /// [`toolbar::arrange`]: drive_app_controls::toolbar::arrange
    pub fn arrange_widths(&self, widths: &[(usize, f32)], bounds: Rect) -> ToolbarArrangement {
        let measures: Vec<ItemMeasure> = widths
            .iter()
            .map(|&(i, width)| ItemMeasure {
                width,
                overflow: match self.inner.items[i].item().overflow {
                    ToolStripItemOverflow::AsNeeded => OverflowBehavior::Auto,
                    ToolStripItemOverflow::Always => OverflowBehavior::Always,
                    ToolStripItemOverflow::Never => OverflowBehavior::Never,
                },
            })
            .collect();

        let a = arrange(
            &measures,
            bounds.right - bounds.left,
            bounds.top,
            self.row_height,
            ITEM_SPACING,
        );
        let shift = |r: Rect| r.shift_x(bounds.left);
        ToolbarArrangement {
            visible: a.visible.into_iter().map(|(k, r)| (widths[k].0, shift(r))).collect(),
            overflow: a.overflow.into_iter().map(|k| widths[k].0).collect(),
            overflow_button: a.overflow_button.map(shift),
        }
    }

    /// Every item's rectangle inside `bounds`, overflow included.
    pub fn item_rects(&self, c: &dyn Canvas, bounds: Rect) -> ToolbarArrangement {
        self.arrange_widths(&self.item_widths(c), bounds)
    }

    /// The item under `(x, y)`, as an index into [`ToolStrip::items`].
    ///
    /// The canvas is unavoidable here and only here: an item's width is a
    /// DirectWrite text extent, so a bar cannot be hit-tested without the very
    /// surface that measured it. [`Toolbar::overflow_at`] answers for the
    /// « More » button separately, because it belongs to no item.
    pub fn item_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        self.item_rects(c, bounds)
            .visible
            .into_iter()
            .find(|(_, r)| r.contains(x, y))
            .map(|(i, _)| i)
    }

    /// Whether `(x, y)` lands on the overflow button.
    pub fn overflow_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> bool {
        self.item_rects(c, bounds).overflow_button.is_some_and(|r| r.contains(x, y))
    }

    /// Paints the bar with one item drawn hot; `hot_overflow` lights the
    /// « More » button, which belongs to no item.
    ///
    /// A `ToolStripItem` is a `Component`: it cannot observe the mouse, and
    /// [`WidgetState`] says only that the *bar* is hot. So the host — which does
    /// know, because it hit-tested — names the item, exactly as the replica's own
    /// `ToolStrip::paint_items` has it name one.
    pub fn paint_items(
        &self,
        c: &dyn Canvas,
        bounds: Rect,
        hot: Option<usize>,
        hot_overflow: bool,
    ) {
        self.paint_with(c, bounds, &StripPaint { hot_overflow, ..StripPaint::hot(hot) });
    }

    /// Paints the bar with its full transient state: hover, press, the keyboard
    /// ring (`:focus-visible` only) and the « More » button's open state.
    pub fn paint_with(&self, c: &dyn Canvas, bounds: Rect, s: &StripPaint) {
        let t = c.theme();
        // The web has no tool bar card: the command bar sits straight on the
        // module panel. No border, no shadow, no radius — the content card just
        // below carries the separation. Drive's own band is opt-in.
        if self.band {
            c.fill_rounded(&bounds, 0.0, &t.layer_background);
        }

        let a = self.item_rects(c, bounds);
        // A bar squeezed below its `Never`-overflow items would spill: clip to
        // the bar, as `overflow-hidden` does.
        c.push_clip(&bounds);
        for (i, rect) in &a.visible {
            let item = &self.inner.items[*i];
            let hot = s.hot == Some(*i) || s.pressed == Some(*i);
            self.paint_item(c, *rect, item, hot);
            if s.pressed == Some(*i) && item.item().enabled && !matches!(visual_of(item), Visual::Separator | Visual::Label) {
                // `active:` — the pressed fill over the hover one.
                c.fill_rounded(rect, pill(rect.bottom - rect.top), &t.control_fill_pressed);
                self.paint_item_ink(c, *rect, item);
            }
            if s.ring(*i) {
                paint_ring(c, *rect, pill(rect.bottom - rect.top));
            }
        }
        if let Some(btn) = a.overflow_button {
            if s.hot_overflow || s.overflow_open {
                c.fill_rounded(&btn, pill(btn.bottom - btn.top), &t.control_fill_hover);
            }
            c.vector_icon("MoreHorizontal", &btn, MORE_ICON, &t.text_secondary);
            if s.ring_overflow() {
                paint_ring(c, btn, pill(btn.bottom - btn.top));
            }
        }
        c.pop_clip();
    }

    /// Re-inks an item's glyphs over a pressed fill (the fill is drawn after
    /// the item so it covers the hover pill, not the icon).
    fn paint_item_ink(&self, c: &dyn Canvas, rect: Rect, item: &StripItem) {
        self.paint_item_inner(c, rect, item, false, false);
    }

    fn paint_item(&self, c: &dyn Canvas, rect: Rect, item: &StripItem, hot: bool) {
        self.paint_item_inner(c, rect, item, hot, true);
    }

    fn paint_item_inner(&self, c: &dyn Canvas, rect: Rect, item: &StripItem, hot: bool, fills: bool) {
        let t = c.theme();
        let f = c.formats();
        let it = item.item();
        let enabled = it.enabled;
        let corner = pill(rect.bottom - rect.top);
        let hot = hot && fills;
        // Command icons are secondary text; disabled only fades them.
        let fg = if enabled { t.text_secondary } else { dimmed(&t.text_secondary) };

        match visual_of(item) {
            Visual::Separator => {
                let mid = (rect.left + rect.right) / 2.0;
                let bar = Rect::new(
                    mid,
                    rect.top + SEPARATOR_INSET,
                    mid + 1.0,
                    rect.bottom - SEPARATOR_INSET,
                );
                c.fill_rounded(&bar, 0.0, &t.divider);
            }
            Visual::Label => {
                c.text_ellipsis_center(&it.text, &rect, &f.body_small, &fg);
            }
            Visual::Icon => {
                if hot && enabled {
                    c.fill_rounded(&rect, corner, &t.control_fill_hover);
                }
                if let Some(name) = item_icon(item) {
                    c.vector_icon(name, &rect, ICON_SIZE, &fg);
                }
            }
            Visual::Toggle => {
                let on = matches!(item, StripItem::Button(b) if b.checked);
                if on {
                    // The web's `bg-primary-light text-primary` pill, NOT a
                    // solid accent block with a white glyph.
                    if fills {
                        c.fill_rounded(&rect, corner, &t.accent_light);
                    }
                } else if hot && enabled {
                    c.fill_rounded(&rect, corner, &t.control_fill_hover);
                }
                if let Some(name) = item_icon(item) {
                    let ink = if on { t.accent } else { fg };
                    c.vector_icon(name, &rect, ICON_SIZE, &ink);
                }
            }
            Visual::IconLabel => {
                if hot && enabled {
                    c.fill_rounded(&rect, corner, &t.control_fill_hover);
                }
                if let Some(name) = item_icon(item) {
                    let icon = Rect::new(
                        rect.left + LABEL_ICON_LEFT,
                        rect.top,
                        rect.left + LABEL_ICON_RIGHT,
                        rect.bottom,
                    );
                    c.vector_icon_layered(name, &icon, ICON_SIZE, &fg, &fg);
                }
                let text = Rect::new(
                    rect.left + LABEL_TEXT_LEFT,
                    rect.top,
                    rect.right - BUTTON_PADDING,
                    rect.bottom,
                );
                c.text_ellipsis(&it.text, &text, &f.body_small, &fg);
            }
            Visual::IconChevron => {
                if hot && enabled {
                    c.fill_rounded(&rect, corner, &t.control_fill_hover);
                }
                if let Some(name) = item_icon(item) {
                    let icon =
                        Rect::new(rect.left, rect.top, rect.right - ICON_CHEVRON_GAP, rect.bottom);
                    c.vector_icon(name, &icon, ICON_SIZE, &fg);
                }
                let chevron = Rect::new(
                    rect.right - ICON_CHEVRON_LEFT,
                    rect.top,
                    rect.right - ICON_CHEVRON_RIGHT,
                    rect.bottom,
                );
                paint_chevron(c, chevron, CHEVRON_DOWN, &t.text_tertiary);
            }
            Visual::MenuLabel => {
                if hot && enabled {
                    c.fill_rounded(&rect, corner, &t.control_fill_hover);
                }
                // The « New » command: accent icon, 14 px label at normal
                // weight, chevron for the menu it opens.
                if let Some(name) = item_icon(item) {
                    let icon = Rect::new(
                        rect.left + LABEL_ICON_LEFT,
                        rect.top,
                        rect.left + LABEL_ICON_RIGHT,
                        rect.bottom,
                    );
                    c.vector_icon(name, &icon, ICON_SIZE, &t.accent);
                }
                let text = Rect::new(
                    rect.left + LABEL_TEXT_LEFT,
                    rect.top,
                    rect.right - MENU_CHEVRON_COLUMN,
                    rect.bottom,
                );
                let ink = if enabled { t.text_primary } else { dimmed(&t.text_primary) };
                c.text_ellipsis(&it.text, &text, &f.body, &ink);
                let chevron = Rect::new(
                    rect.right - MENU_CHEVRON_COLUMN,
                    rect.top,
                    rect.right - MENU_CHEVRON_INSET,
                    rect.bottom,
                );
                paint_chevron(c, chevron, CHEVRON_DOWN, &t.text_tertiary);
            }
        }
    }
}

/// A chevron, centred in `rect` at the size `formats().icon_small` would have
/// drawn it. Rule 6: geometry, never a codepoint.
fn paint_chevron(c: &dyn Canvas, rect: Rect, name: &'static str, color: &D2D1_COLOR_F) {
    c.vector_icon(name, &rect, CHEVRON_SIZE, color);
}

impl Deref for Toolbar {
    type Target = ToolStrip;
    fn deref(&self) -> &ToolStrip {
        &self.inner
    }
}
impl DerefMut for Toolbar {
    fn deref_mut(&mut self) -> &mut ToolStrip {
        &mut self.inner
    }
}

impl Widget for Toolbar {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, c: &dyn Canvas) -> Size {
        let widths = self.item_widths(c);
        let total: f32 = widths.iter().map(|&(_, w)| w).sum::<f32>()
            + ITEM_SPACING * widths.len().saturating_sub(1) as f32;
        Size::new(total, self.row_height)
    }

    fn paint(&self, c: &dyn Canvas, bounds: Rect, _state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        c.fill_rounded(&bounds, 0.0, &c.current_bg());
        self.paint_items(c, bounds, None, false);
    }

    fn type_name(&self) -> &'static str {
        "Toolbar"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Sidebar
// ═════════════════════════════════════════════════════════════════════════════

/// The Kubuno navigation pane.
///
/// # Why a vertical `ToolStrip` and not a list
///
/// A list box models *one homogeneous collection with a selected index*. A
/// sidebar is not that: its rows are of two kinds (navigation rows and inert
/// section headers), each carries an icon and an enabled state, each can be
/// hidden, and a header folds the rows under it. Every one of those is already
/// a field on `ToolStripItem` — `Image`, `Enabled`, `Visible`, and the item
/// *chain* itself for the two kinds — whereas `ListBox` would need all of them
/// added on the Kubuno side, which is exactly the duplication the brief
/// forbids. The stacking is the replica's too: [`layout_items`] on
/// [`StripAxis::Vertical`] packs the rows, honours each item's `Margin`, and is
/// where the section gap and the row gap live, so this file computes no offsets
/// of its own.
///
/// The active row is `ToolStripButton::Checked`, which is what a checked strip
/// button already means; the pill it wears is the web's `bg-primary-light` /
/// `text-nav-active` pair, not the toolkit's `Highlight`.
#[derive(Clone)]
pub struct Sidebar {
    inner: ToolStrip,
    /// Docked pane, icon rail, or floating overlay — the pane state
    /// `SidebarDisplayMode` describes and .NET has no counterpart for.
    pub mode: SidebarMode,
    /// Horizontal content offset when the labels are wider than the pane
    /// (`SidebarView::dx`); always 0 on the Compact rail.
    pub dx: f32,
    /// `Some(expanded)` per row, index-aligned with [`ToolStrip::items`]: the
    /// one thing the item chain cannot say, since a `ToolStripDropDownItem`
    /// declares its children but never whether they are shown. A missing entry
    /// means « this row has no chevron ».
    pub expanded: Vec<Option<bool>>,
    /// Vertical scroll offset of the rows, in DIP: a pane whose rows do not
    /// fit scrolls (`overflow-y-auto`), it never folds rows away. 0 = top;
    /// clamp it with [`Sidebar::max_scroll`].
    pub scroll_y: f32,
}

impl Default for Sidebar {
    fn default() -> Self {
        Self::new()
    }
}

impl Sidebar {
    pub fn new() -> Self {
        // A sidebar is a `ToolStrip` with four different defaults, the same way
        // `MenuStrip` and `StatusStrip` are: it stacks downwards, docks to the
        // left, shows no move handle, and never overflows into a menu (a row
        // that does not fit is scrolled to, not folded away).
        let mut inner = ToolStrip::new();
        inner.grip_style = ToolStripGripStyle::Hidden;
        inner.layout_style = ToolStripLayoutStyle::VerticalStackWithOverflow;
        inner.can_overflow = false;
        inner.control_mut().dock = kubuno_controls::enums::DockStyle::Left;
        inner.image_scaling_size = Size::new(ROW_ICON_SIZE, ROW_ICON_SIZE);
        Self { inner, mode: SidebarMode::Expanded, dx: 0.0, expanded: Vec::new(), scroll_y: 0.0 }
    }

    /// The rows' total height — what the pane would need to show them all.
    pub fn content_height(&self) -> f32 {
        self.row_inputs().iter().filter(|i| i.visible).map(|i| i.size.height).sum()
    }

    /// The largest meaningful [`Sidebar::scroll_y`] for a pane of `bounds`.
    pub fn max_scroll(&self, bounds: Rect) -> f32 {
        (self.content_height() - (bounds.bottom - bounds.top)).max(0.0)
    }

    /// Whether row `i` can take the keyboard focus: a visible, enabled
    /// navigation row (section headers are inert).
    pub fn is_focusable(&self, i: usize) -> bool {
        self.inner
            .items
            .get(i)
            .is_some_and(|it| it.item().visible && it.item().enabled && !Self::is_section(it))
    }

    /// The row the arrows move the focus to (Up / Down / Home / End, no
    /// wrap — a tree does not wrap).
    pub fn step_focus(&self, from: Option<usize>, key: NavKey) -> Option<usize> {
        roving_step(self.inner.items.len(), from, key, false, |i| self.is_focusable(i))
    }

    /// The scroll offset that brings row `i` fully into view in a pane of
    /// `bounds`, starting from the current [`Sidebar::scroll_y`].
    pub fn reveal_row(&self, bounds: Rect, i: usize) -> f32 {
        let unscrolled = Sidebar { scroll_y: 0.0, ..self.clone() };
        let Some(r) = unscrolled.row_rects(bounds).get(i).copied() else { return self.scroll_y };
        let top = r.top - bounds.top;
        let bottom = r.bottom - bounds.top;
        let h = bounds.bottom - bounds.top;
        let s = if top < self.scroll_y {
            top
        } else if bottom > self.scroll_y + h {
            bottom - h
        } else {
            self.scroll_y
        };
        s.clamp(0.0, self.max_scroll(bounds))
    }

    /// Adds a row (and, optionally, its chevron state).
    pub fn with(mut self, item: StripItem, chevron: Option<bool>) -> Self {
        self.inner.items.push(item);
        self.expanded.push(chevron);
        self
    }

    fn chevron(&self, i: usize) -> Option<bool> {
        self.expanded.get(i).copied().flatten()
    }

    /// Whether row `i` is an inert section header.
    fn is_section(item: &StripItem) -> bool {
        matches!(item, StripItem::Label(_))
    }

    /// The margin one row reserves around itself.
    ///
    /// * `space::SM` left and right — the shipping pane insets its rows by 8
    ///   from each wall (`drive-app/src/ui/layout.rs`).
    /// * `space::XXS` below — the 2 DIP the same code puts between two rows.
    /// * [`SIDEBAR_SECTION_GAP`] above a section header that follows another
    ///   row — `FlatSidebarItem::SectionGapMargin`, the only reason a gap ever
    ///   grows.
    fn row_margin(&self, i: usize) -> Padding {
        let top = if i > 0 && Self::is_section(&self.inner.items[i]) {
            SIDEBAR_SECTION_GAP
        } else {
            0.0
        };
        Padding::new(space::SM, top, space::SM, space::XXS)
    }

    /// The inputs [`layout_items`] packs — one per row, outer size (content
    /// plus margin) on the stacking axis.
    fn row_inputs(&self) -> Vec<StripItemInput> {
        self.inner
            .items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let m = self.row_margin(i);
                StripItemInput {
                    size: Size::new(0.0, SIDEBAR_ROW_HEIGHT + m.vertical()),
                    alignment: it.item().alignment,
                    overflow: it.item().overflow,
                    spring: false,
                    visible: it.item().visible,
                }
            })
            .collect()
    }

    /// Every row's rectangle inside `bounds`.
    ///
    /// Canvas-free: a row's height is a token and its width is the pane's, so
    /// nothing has to be measured. The stacking itself is [`layout_items`].
    pub fn row_rects(&self, bounds: Rect) -> Vec<Rect> {
        let layout = layout_items(
            StripAxis::Vertical,
            bounds,
            0.0,
            0.0,
            self.inner.can_overflow,
            false,
            &self.row_inputs(),
        );
        layout
            .rects
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let r = inset(r, self.row_margin(i));
                Rect::new(r.left, r.top - self.scroll_y, r.right, r.bottom - self.scroll_y)
            })
            .collect()
    }

    /// The row under `(x, y)` — no canvas needed. A row scrolled out of the
    /// pane is not under anything.
    pub fn item_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        if !bounds.contains(x, y) {
            return None;
        }
        self.row_rects(bounds)
            .into_iter()
            .enumerate()
            .find(|(i, r)| {
                self.inner.items[*i].item().visible && r.right > r.left && r.contains(x, y)
            })
            .map(|(i, _)| i)
    }

    /// The pane's width in its current mode — [`sidebar_pane_width`], with the
    /// replica's own `Size.Width` as the stored Expanded preference.
    pub fn pane_width(&self) -> f32 {
        sidebar_pane_width(self.mode, self.inner.control().size().width)
    }

    /// The natural width of a row: `8 + indent + icon + gap + label + margin`,
    /// the same sum `sidebar_row_width` reports — used to decide whether the
    /// pane needs a horizontal scroll.
    pub fn row_width(&self, c: &dyn Canvas, i: usize) -> f32 {
        let Some(item) = self.inner.items.get(i) else { return 0.0 };
        let text = c.measure(&item.item().text, &c.formats().body);
        drive_app_controls::sidebar::sidebar_row_width(item.item().padding.left, text)
    }

    /// Paints the pane with one row drawn hot.
    pub fn paint_rows(&self, c: &dyn Canvas, bounds: Rect, hot: Option<usize>) {
        self.paint_with(c, bounds, &StripPaint::hot(hot));
    }

    /// Paints the pane with its full transient state: hover, press, and the
    /// keyboard ring around the focused row (`:focus-visible` only).
    pub fn paint_with(&self, c: &dyn Canvas, bounds: Rect, s: &StripPaint) {
        let hot = s.hot;
        let t = c.theme();
        let f = c.formats();
        let compact = self.mode == SidebarMode::Compact;

        // Minimal is a FLOATING pane: it wears the flyout surface and its
        // shadow, painted before the clip so the rows land on top.
        if self.mode == SidebarMode::Minimal {
            c.draw_card_shadow(&bounds, radius::XL);
            c.fill_rounded(&bounds, radius::XL, &t.flyout_background);
            c.stroke_rounded(&bounds, radius::XL, &t.card_stroke);
        }

        c.push_clip(&bounds);
        let rects = self.row_rects(bounds);
        for (i, item) in self.inner.items.iter().enumerate() {
            let rect = rects[i];
            if !item.item().visible || rect.right <= rect.left {
                continue;
            }
            let it = item.item();
            let section = Self::is_section(item);
            let selected = matches!(item, StripItem::Button(b) if b.checked);
            let indent = it.padding.left;

            // A full pill (`rounded-full`), recomputed from the row's own height
            // so the Compact rail still reads as a centred stadium. The ACTIVE
            // row keeps its pastille on hover — the two fills never stack.
            let corner = pill(rect.bottom - rect.top);
            let live = !section && it.enabled;
            if selected {
                c.fill_rounded(&rect, corner, &t.accent_light);
            } else if s.pressed == Some(i) && live {
                c.fill_rounded(&rect, corner, &t.control_fill_pressed);
            } else if hot == Some(i) && live {
                c.fill_rounded(&rect, corner, &t.control_fill_hover);
            }
            if s.ring(i) && live {
                paint_ring(c, rect, corner);
            }

            let icon_left = if compact {
                rect.left + ((rect.right - rect.left) - ROW_ICON_SIZE) / 2.0
            } else {
                rect.left + indent + self.dx
            };
            let icon_rect =
                Rect::new(icon_left, rect.top, icon_left + ROW_ICON_SIZE, rect.bottom);
            // Nav icons rest in the secondary colour and take the accent on the
            // active row.
            let mut ink = if selected { t.accent } else { t.text_secondary };
            if !it.enabled {
                ink = dimmed(&ink);
            }
            if let Some(name) = item_icon(item) {
                c.vector_icon(name, &icon_rect, SIDEBAR_ICON_GLYPH, &ink);
            }

            if compact {
                continue;
            }

            let label_rect = Rect::new(
                icon_rect.right + ROW_ICON_TEXT_GAP,
                rect.top,
                rect.right - ROW_TEXT_RIGHT_MARGIN,
                rect.bottom,
            );
            // Section header: 12 px semi-bold UPPERCASE in `text_tertiary`.
            // Nav row: 14 px, `text_nav_active` when active, else `text_primary`.
            let (format, color) = if section {
                (&f.caption_strong, t.text_tertiary)
            } else if selected {
                (&f.body, t.text_nav_active)
            } else {
                (&f.body, t.text_primary)
            };
            let color = if it.enabled { color } else { dimmed(&color) };
            let label: Cow<str> = if section {
                it.text.to_uppercase().into()
            } else {
                it.text.as_str().into()
            };
            c.text_ellipsis(&label, &label_rect, format, &color);

            if let Some(open) = self.chevron(i) {
                let glyph = if open { CHEVRON_DOWN } else { CHEVRON_RIGHT };
                let chevron_rect = Rect::new(
                    rect.left + indent - SIDEBAR_CHEVRON_LEFT + self.dx,
                    rect.top,
                    rect.left + indent - SIDEBAR_CHEVRON_RIGHT + self.dx,
                    rect.bottom,
                );
                let tint = if section { t.text_tertiary } else { t.text_secondary };
                paint_chevron(c, chevron_rect, glyph, &tint);
            }
        }
        c.pop_clip();
    }
}

impl Deref for Sidebar {
    type Target = ToolStrip;
    fn deref(&self) -> &ToolStrip {
        &self.inner
    }
}
impl DerefMut for Sidebar {
    fn deref_mut(&mut self) -> &mut ToolStrip {
        &mut self.inner
    }
}

impl Widget for Sidebar {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, _c: &dyn Canvas) -> Size {
        let height: f32 = self.row_inputs().iter().filter(|i| i.visible).map(|i| i.size.height).sum();
        Size::new(self.pane_width(), height)
    }

    fn paint(&self, c: &dyn Canvas, bounds: Rect, _state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        c.fill_rounded(&bounds, 0.0, &c.current_bg());
        self.paint_rows(c, bounds, None);
    }

    fn type_name(&self) -> &'static str {
        "Sidebar"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Breadcrumb
// ═════════════════════════════════════════════════════════════════════════════

/// The Kubuno path trail.
///
/// The hard part of a breadcrumb — deciding which head segments to fold behind
/// a `…` when the path no longer fits, always keeping the current folder
/// visible — is [`layout_breadcrumbs`], ported from `BreadcrumbBarLayout` and
/// tested there. This primitive **wraps** it: it measures the segments, hands
/// them over, and paints what comes back. Nothing about the folding is
/// re-derived here, which is the point.
#[derive(Clone)]
pub struct Breadcrumb {
    inner: ToolStrip,
    /// Whether the trail opens with a separator, because the host draws a home
    /// icon in front of it (`BreadcrumbView::home_chevron`). It costs one
    /// [`CHEVRON_BLOCK`] at the leading edge.
    pub root_chevron: bool,
}

impl Default for Breadcrumb {
    fn default() -> Self {
        Self::new()
    }
}

impl Breadcrumb {
    pub fn new() -> Self {
        let mut inner = ToolStrip::new();
        inner.grip_style = ToolStripGripStyle::Hidden;
        inner.layout_style = ToolStripLayoutStyle::HorizontalStackWithOverflow;
        Self { inner, root_chevron: false }
    }

    /// Appends a segment.
    pub fn with(mut self, text: &str) -> Self {
        let mut b = ToolStripButton::new(text);
        b.item.display_style = ToolStripItemDisplayStyle::Text;
        self.inner.items.push(StripItem::Button(b));
        self
    }

    /// The leading chevron's own block, when [`Breadcrumb::root_chevron`] is on.
    pub fn root_chevron_rect(&self, bounds: Rect) -> Option<Rect> {
        self.root_chevron.then(|| {
            Rect::new(
                bounds.left + CHEVRON_OFFSET,
                bounds.top,
                bounds.left + CHEVRON_OFFSET + CHEVRON_SIZE,
                bounds.bottom,
            )
        })
    }

    /// Each segment's total width — its label (`text-sm font-medium`, capped
    /// at `maxSegmentWidth`, beyond which it truncates) plus
    /// `BreadcrumbBarItemPadding`.
    pub fn segment_widths(&self, c: &dyn Canvas) -> Vec<f32> {
        let f = c.formats();
        self.inner
            .items
            .iter()
            .map(|it| text_width(c, &it.item().text, &f.body_strong).min(SEGMENT_MAX_TEXT) + SEGMENT_PADDING)
            .collect()
    }

    /// Whether segment `i` is a link — every segment but the last: « the page
    /// you are on is not a destination » (`Breadcrumb.tsx`).
    pub fn is_link(&self, i: usize) -> bool {
        i + 1 < self.inner.items.len() && self.inner.items[i].item().enabled
    }

    /// The segments folded behind the `…` button in `l`, as indices into
    /// [`ToolStrip::items`] (the head of the path: `0 .. start_index`).
    pub fn hidden_segments(&self, l: &BreadcrumbLayout) -> std::ops::Range<usize> {
        if l.ellipsis.is_some() {
            0..l.start_index.min(self.inner.items.len())
        } else {
            0..0
        }
    }

    /// The menu the `…` button opens: the folded segments, root first — the
    /// web's `hiddenItems` fed to `MenuDropdown`. Row `k` stands for segment
    /// `hidden_segments(l).start + k`.
    pub fn hidden_menu(&self, l: &BreadcrumbLayout) -> Menu {
        let rows = self
            .hidden_segments(l)
            .map(|i| {
                MenuEntry::new(self.inner.items[i].item().text.clone())
                    .icon("Folder")
                    .enabled(self.inner.items[i].item().enabled)
                    .build()
            })
            .collect();
        Menu::with_items(rows)
    }

    /// The trail's keyboard stops, in visual order: the `…` button, then each
    /// drawn LINK segment (the current page is not a link and is not a stop).
    /// Unlike a tool bar these are ordinary Tab stops — a trail is a list of
    /// links, not a composite widget.
    pub fn focus_order(&self, l: &BreadcrumbLayout) -> Vec<Option<usize>> {
        let mut v = Vec::new();
        if l.ellipsis.is_some() {
            v.push(None);
        }
        for k in 0..l.items.len() {
            let i = l.start_index + k;
            if self.is_link(i) {
                v.push(Some(i));
            }
        }
        v
    }

    /// Folds and places already-measured segments. Pure: it is exactly one call
    /// to [`layout_breadcrumbs`] with the parameters `drive-app` fills in.
    pub fn layout_with(&self, widths: &[f32], bounds: Rect) -> BreadcrumbLayout {
        let start_x = bounds.left + if self.root_chevron { CHEVRON_BLOCK } else { 0.0 };
        layout_breadcrumbs(
            widths,
            &BreadcrumbLayoutParams {
                start_x,
                avail_right: bounds.right,
                top: bounds.top,
                bottom: bounds.bottom,
                chevron_block: CHEVRON_BLOCK,
                ellipsis_width: ELLIPSIS_WIDTH,
            },
        )
    }

    /// The trail's geometry inside `bounds`.
    pub fn layout(&self, c: &dyn Canvas, bounds: Rect) -> BreadcrumbLayout {
        self.layout_with(&self.segment_widths(c), bounds)
    }

    /// The segment under `(x, y)`, as an index into [`ToolStrip::items`] — the
    /// original collection, so a folded path still reports the real segment.
    pub fn item_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let l = self.layout(c, bounds);
        l.items
            .iter()
            .position(|r| r.contains(x, y))
            .map(|k| l.start_index + k)
    }

    /// Whether `(x, y)` lands on the `…` button.
    pub fn ellipsis_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> bool {
        self.layout(c, bounds).ellipsis.is_some_and(|r| r.contains(x, y))
    }

    /// Paints the trail with one segment hot; `hot_ellipsis` lights the `…`.
    pub fn paint_segments(
        &self,
        c: &dyn Canvas,
        bounds: Rect,
        hot: Option<usize>,
        hot_ellipsis: bool,
    ) {
        self.paint_with(c, bounds, &StripPaint { hot_overflow: hot_ellipsis, ..StripPaint::hot(hot) });
    }

    /// Paints the trail with its full transient state.
    ///
    /// `Breadcrumb.tsx`: a link segment is `text-text-secondary
    /// hover:text-primary` — the hover answers in the TEXT colour, there is no
    /// fill — and the current page is `text-text-primary`, not a link, with no
    /// hover at all. The keyboard ring is `focus-visible:ring-2 ring-primary
    /// rounded-sm`. Chevrons are drawn BETWEEN segments only (`index > 0`),
    /// in `text-text-tertiary`.
    pub fn paint_with(&self, c: &dyn Canvas, bounds: Rect, s: &StripPaint) {
        let t = c.theme();
        let f = c.formats();
        let l = self.layout(c, bounds);
        c.push_clip(&bounds);

        if let Some(r) = self.root_chevron_rect(bounds) {
            paint_chevron(c, r, CHEVRON_RIGHT, &t.text_tertiary);
        }

        if let Some(ell) = l.ellipsis {
            // The `…` is a link-styled button too: secondary, primary on hover
            // or while its menu is open.
            let lit = s.hot_overflow || s.overflow_open;
            let ink = if lit { t.accent } else { t.text_secondary };
            c.vector_icon("MoreHorizontal", &ell, MORE_ICON, &ink);
            if s.ring_overflow() {
                paint_ring(c, ell, radius::SM);
            }
            let chevron = Rect::new(
                ell.right + CHEVRON_OFFSET,
                ell.top,
                ell.right + CHEVRON_OFFSET + CHEVRON_SIZE,
                ell.bottom,
            );
            paint_chevron(c, chevron, CHEVRON_RIGHT, &t.text_tertiary);
        }

        // The layout always keeps the TAIL visible, so the last drawn segment
        // IS the current folder.
        let last = self.inner.items.len().saturating_sub(1);
        let drawn = l.items.len();
        for (k, rect) in l.items.iter().enumerate() {
            let i = l.start_index + k;
            let Some(item) = self.inner.items.get(i) else { continue };
            let link = self.is_link(i);
            let color = if i == last {
                t.text_primary
            } else if link && (s.hot == Some(i) || s.pressed == Some(i)) {
                t.accent
            } else {
                t.text_secondary
            };
            let color = if item.item().enabled { color } else { dimmed(&color) };
            let label = Rect::new(
                rect.left + SEGMENT_TEXT_LEFT,
                rect.top,
                rect.right - (SEGMENT_PADDING - SEGMENT_TEXT_LEFT),
                rect.bottom,
            );
            c.text_ellipsis(&item.item().text, &label, &f.body_strong, &color);
            if link && s.ring(i) {
                paint_ring(c, *rect, radius::SM);
            }
            if k + 1 < drawn {
                let chevron = Rect::new(
                    rect.right + CHEVRON_OFFSET,
                    rect.top,
                    rect.right + CHEVRON_OFFSET + CHEVRON_SIZE,
                    rect.bottom,
                );
                paint_chevron(c, chevron, CHEVRON_RIGHT, &t.text_tertiary);
            }
        }
        c.pop_clip();
    }
}

impl Deref for Breadcrumb {
    type Target = ToolStrip;
    fn deref(&self) -> &ToolStrip {
        &self.inner
    }
}
impl DerefMut for Breadcrumb {
    fn deref_mut(&mut self) -> &mut ToolStrip {
        &mut self.inner
    }
}

impl Widget for Breadcrumb {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, c: &dyn Canvas) -> Size {
        let widths = self.segment_widths(c);
        let mut width: f32 = widths.iter().sum::<f32>()
            + CHEVRON_BLOCK * widths.len().saturating_sub(1) as f32;
        if self.root_chevron {
            width += CHEVRON_BLOCK;
        }
        // The trail lives inside the omnibar, whose height is the design
        // system's input height.
        Size::new(width, height::BUTTON_MD)
    }

    fn paint(&self, c: &dyn Canvas, bounds: Rect, _state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        c.fill_rounded(&bounds, 0.0, &c.current_bg());
        self.paint_segments(c, bounds, None, false);
    }

    fn type_name(&self) -> &'static str {
        "Breadcrumb"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tabs
// ═════════════════════════════════════════════════════════════════════════════

/// The Kubuno tab strip.
///
/// # Which tab style this is, and why
///
/// The product already draws tabs in two different shapes, and they are not
/// interchangeable:
///
/// * the Drive **title-bar** strip is chrome-like — top corners at 8, concave
///   4 DIP feet, merged with the tool bar into one translucent surface. That
///   shape exists to weld the tab to the window chrome; it is a window
///   decoration, not a design-system primitive.
/// * every **in-content** selector — the Drive info pane's « Détails / Aperçu »,
///   which is a direct port of `@ui/Tabs.tsx` — is the web's `underline`
///   variant: a `divider` hairline under the whole strip, a 3 DIP accent band
///   under the active tab inset by 8 (`mx-2`, `rounded-t-[3px]`), accent text at
///   medium weight, `surface_2` on hover.
///
/// This primitive is the second one. It is the variant `Tabs.tsx` defaults to,
/// it is the one the crate's own metric table already reserved
/// ([`control::TAB_MD`], [`control::TAB_UNDERLINE`]), and it is the only one
/// that makes sense inside a panel. The chrome strip stays where it belongs —
/// in the shell that owns the window frame.
///
/// The model is [`TabControl`]: `SelectedIndex`, `Alignment`, `Multiline` and
/// `SizeMode` are the replica's, and so is the wrapping — [`tab_strip`],
/// [`tab_rows`] and [`tab_rects`] place the tabs, including the row rotation
/// that puts the selected tab's row against the page.
#[derive(Clone)]
pub struct Tabs {
    inner: TabControl,
    /// Horizontal scroll of an overflowing single-row strip, in DIP.
    ///
    /// `None` — the default, for a caller that keeps no state — scrolls just
    /// enough to keep the SELECTED tab in view (`revealActive`), so a strip
    /// too narrow for its tabs never hides the one the page below belongs to.
    /// `Some(x)` is a caller-driven offset (the scroll arrows, the wheel);
    /// it is clamped, never trusted. See [`Tabs::scroll_state`].
    pub scroll: Option<f32>,
}

/// The scroll state of a single-row strip whose tabs do not all fit.
#[derive(Clone, Copy)]
pub struct TabScroll {
    /// The applied offset, clamped to `0 ..= max`.
    pub offset: f32,
    /// The largest offset — content width minus the viewport's.
    pub max: f32,
    /// The band the tabs are visible in: the strip minus the arrows.
    pub viewport: Rect,
    /// `ChevronLeft`, shown only when the strip can scroll back
    /// (`canScroll.left`).
    pub left_arrow: Option<Rect>,
    /// `ChevronRight`, shown only when it can scroll further.
    pub right_arrow: Option<Rect>,
}

/// What the host knows about a tab strip this frame, for [`Tabs::paint_with`].
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TabsPaint {
    /// The tab under the pointer.
    pub hot: Option<usize>,
    /// The tab the left button is held on.
    pub pressed: Option<usize>,
    /// The tab holding the keyboard focus — in a tab list that is the
    /// selected one (the roving tab stop).
    pub focused: Option<usize>,
    /// `:focus-visible` — paint the ring around [`TabsPaint::focused`].
    pub focus_visible: bool,
    /// The scroll arrow under the pointer: `-1` left, `1` right.
    pub hot_arrow: Option<i32>,
    /// Where the travelling indicator is, as a CONTENT-space span `(left,
    /// right)` of the tab it currently covers (see [`Tabs::content_span`]).
    /// `None` puts it under the selected tab. A caller animating a tab change
    /// interpolates between the two tabs' spans with [`ease_span`].
    pub indicator: Option<(f32, f32)>,
}

/// The indicator's slide: `transition-[transform,width] duration-200
/// ease-out` (`Tabs.tsx`).
pub const TAB_SLIDE_MS: u32 = 200;

/// The indicator span at progress `t` (0 → 1) of a slide from `from` to
/// `to`, eased out (cubic) as CSS's `ease-out` approximately is.
pub fn ease_span(from: (f32, f32), to: (f32, f32), t: f32) -> (f32, f32) {
    let t = t.clamp(0.0, 1.0);
    let e = 1.0 - (1.0 - t).powi(3);
    (from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e)
}

/// An indicator slide: `(from, to, start_ms)`, content-space spans.
type TabSlide = ((f32, f32), (f32, f32), u64);

/// What a live tab strip keeps between frames — what `Tabs.tsx` keeps in its
/// own hooks: the travelling indicator's slide, the scroll offset and the
/// press edge.
///
/// A caller keeps ONE per strip across frames and runs every frame through
/// [`TabsController::frame`]. That is the point of it: every strip — an app's,
/// a demo page's, the component gallery's own navigation — gets the
/// component's whole behaviour (sliding indicator, scroll arrows, wheel,
/// keyboard, reveal of the selected tab) from the component itself, instead of
/// each caller hand-copying the subset it remembered.
#[derive(Debug, Default, Clone)]
pub struct TabsController {
    /// `(from, to, start_ms)`: the indicator's slide, as content-space spans.
    slide:     Option<TabSlide>,
    /// The strip's scroll offset, written into [`Tabs::scroll`] each frame.
    scroll:    Option<f32>,
    /// The selection the strip showed last frame, so a change made OUTSIDE the
    /// controller (a keyboard shortcut, a program) slides the indicator too.
    last:      Option<i32>,
    prev_down: bool,
    /// One scrolled page area per tab, by index, so each page keeps its own
    /// scroll position across switches — as the web keeps each panel's.
    pages:     Vec<crate::containers::ScrollArea>,
}

/// What one [`TabsController::frame`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabsFrame {
    /// The selection changed this frame (click or keyboard).
    pub changed: bool,
    /// The tab under the pointer.
    pub hot:     Option<usize>,
}

impl TabsController {
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the indicator is still sliding — a caller that paints on demand
    /// needs no more than this: the controller already asks the host for the
    /// next frame while it moves.
    pub fn is_animating(&self) -> bool {
        self.slide.is_some()
    }

    /// Paints the selected tab's page into `page` (the page area, see
    /// [`Tabs::page_rect`]) inside a [`crate::containers::ScrollArea`] of its
    /// own: whatever the page paints past the area's edges brings up a
    /// vertical or horizontal bar, by default, and the page scrolls with the
    /// wheel and the bars. Each tab remembers its own scroll.
    ///
    /// `paint` lays the page out in the same coordinates it would use without
    /// scrolling, and receives the [`host::Frame`] translated to them.
    pub fn page(
        &mut self,
        c: &dyn Canvas,
        tabs: &Tabs,
        page: Rect,
        f: &host::Frame,
        paint: impl FnOnce(&dyn Canvas, &host::Frame),
    ) -> crate::containers::ScrollAreaRun {
        let i = usize::try_from(tabs.selected_index).unwrap_or(0);
        if self.pages.len() <= i {
            self.pages.resize_with(i + 1, Default::default);
        }
        self.pages[i].frame(c, page, f, paint)
    }

    /// Runs one frame of a live strip in `bounds`, then paints it.
    ///
    /// * a click on a tab selects it; on a scroll arrow, it pages the strip;
    /// * the wheel over the strip scrolls it (horizontal wheel, or vertical
    ///   with Shift);
    /// * while `focus` says the strip holds the keyboard focus, the axis arrows,
    ///   Home and End move the selection — activation follows focus, as in the
    ///   web's tab list — and the ring shows on `:focus-visible`;
    /// * a selection change scrolls the new tab into view and slides the
    ///   indicator to it (`TAB_SLIDE_MS`, eased out), from wherever it is,
    ///   mid-slide included, repainting until it lands.
    ///
    /// `tabs.selected_index` is read and written; `tabs.scroll` is owned here.
    pub fn frame(
        &mut self,
        c: &dyn Canvas,
        tabs: &mut Tabs,
        bounds: Rect,
        f: &host::Frame,
        focus: Option<crate::focus::FocusState>,
    ) -> TabsFrame {
        let now = host::now_ms();
        let (mx, my) = f.mouse;
        let clicked = f.mouse_down && !self.prev_down;
        self.prev_down = f.mouse_down;
        tabs.scroll = self.scroll;
        let old = self.last.unwrap_or(tabs.selected_index);

        let arrow = tabs.arrow_at(c, bounds, mx, my);
        if clicked {
            if let Some(dir) = arrow {
                self.scroll = Some(tabs.scrolled_by_page(c, bounds, dir));
            } else if let Some(i) = tabs.item_at(c, bounds, mx, my) {
                tabs.selected_index = i as i32;
            }
        }
        if bounds.contains(mx, my) {
            let (wx, wy) = f.wheel_dip();
            let along = if wx != 0.0 { wx } else if f.mods.shift { wy } else { 0.0 };
            if along != 0.0 {
                tabs.scroll = self.scroll;
                if let Some(s) = tabs.scroll_state(c, bounds) {
                    self.scroll = Some((s.offset + along * 0.5).clamp(0.0, s.max));
                }
            }
        }
        let focused = focus.is_some_and(|s| s.focused);
        if focused {
            for key in take_nav_keys(tabs.is_vertical()) {
                if let Some(i) = tabs.step_selection(key) {
                    tabs.selected_index = i as i32;
                }
            }
        }

        let changed = tabs.selected_index != old;
        if changed && old >= 0 && tabs.selected_index >= 0 {
            let new = tabs.selected_index as usize;
            let spans = (tabs.content_span(c, bounds, old as usize), tabs.content_span(c, bounds, new));
            if let (Some(from), Some(to)) = spans {
                // A second change mid-slide starts from where the bar IS, not
                // from where it was going: it never jumps.
                let from = match self.slide {
                    Some((a, b, start)) => {
                        ease_span(a, b, now.saturating_sub(start) as f32 / TAB_SLIDE_MS as f32)
                    }
                    None => from,
                };
                self.slide = Some((from, to, now));
            }
            tabs.scroll = self.scroll;
            self.scroll = Some(tabs.reveal_offset(c, bounds));
        }
        tabs.scroll = self.scroll;

        let indicator = match self.slide {
            Some((a, b, start)) => {
                let t = now.saturating_sub(start) as f32 / TAB_SLIDE_MS as f32;
                if t >= 1.0 {
                    self.slide = None;
                    None
                } else {
                    host::request_repaint_after(16);
                    Some(ease_span(a, b, t))
                }
            }
            None => None,
        };
        let hot = tabs.item_at(c, bounds, mx, my);
        tabs.paint_with(
            c,
            bounds,
            &TabsPaint {
                hot,
                pressed: hot.filter(|_| f.mouse_down),
                focused: usize::try_from(tabs.selected_index).ok(),
                focus_visible: focus.is_some_and(|s| s.focused && s.visible),
                hot_arrow: arrow,
                indicator,
            },
        );
        self.last = Some(tabs.selected_index);
        TabsFrame { changed, hot }
    }
}

impl Default for Tabs {
    fn default() -> Self {
        Self::new()
    }
}

impl Tabs {
    pub fn new() -> Self {
        let mut inner = TabControl::new();
        // `Tabs.tsx` size `md`: `px-4 h-12`. The replica's WinForms default
        // (6, 3) is a system metric, not a design token.
        inner.padding = Size::new(space::LG, TAB_PAD_Y_MD);
        Self { inner, scroll: None }
    }

    /// The web's `sm` size: `px-3 pt-1.5 pb-[9px]`. There is no Kubuno field
    /// for it — the size *is* the replica's `Padding`, which is what both axes
    /// are measured from, so the two can never disagree.
    pub fn small(mut self) -> Self {
        self.inner.padding = Size::new(space::MD, TAB_PAD_Y_SM);
        self
    }

    /// Appends a tab, selecting the first one as `TabPages.Add` does.
    pub fn with(mut self, text: &str) -> Self {
        self.inner.add_page(TabPage::new(text));
        self
    }

    /// Whether the strip runs down a side (Left / Right alignment) — which
    /// also says which arrows move between its tabs.
    pub fn is_vertical(&self) -> bool {
        strip_is_vertical(self.inner.alignment)
    }

    /// The web's `underline` strip: one row, along the top or the bottom. It
    /// is the only arrangement that scrolls instead of wrapping, and the only
    /// one whose tabs share the widest one's width.
    fn scroller_mode(&self) -> bool {
        !self.inner.multiline && !self.is_vertical()
    }

    /// One tab row's thickness.
    ///
    /// `ItemSize.Height` still wins when the model sets it — that is the
    /// replica's property and it must keep meaning what it means. Left auto,
    /// the `md` row is the web's stated `h-12` and the `sm` row is what its
    /// paddings build: `pt-1.5`, the 20 DIP line, `pb-[9px]` (the 6 again
    /// plus the 3 the indicator takes back) — [`control::TAB_SM`].
    pub fn row_thickness(&self) -> f32 {
        if self.inner.item_size.height > 0.0 {
            self.inner.item_size.height
        } else if (self.inner.padding.height - TAB_PAD_Y_MD).abs() < f32::EPSILON {
            TAB_HEIGHT_MD
        } else {
            2.0 * self.inner.padding.height + TAB_TEXT_LINE + control::TAB_UNDERLINE
        }
    }

    /// Each tab's along-axis extent, before wrapping — the replica's
    /// [`tab_strip`], fed the measured captions.
    pub fn extents_with(&self, labels: &[f32], bounds: Rect) -> Vec<f32> {
        tab_strip(
            labels,
            self.inner.padding.width,
            self.inner.item_size.width,
            self.inner.size_mode,
            self.along_extent(bounds),
        )
        .iter()
        .map(|t| t.1)
        .collect()
    }

    /// The extents actually laid out. In the underline strip « every tab gets
    /// the width of the WIDEST one » (`grid auto-cols-fr w-max`), so a label
    /// change never shifts its neighbours; the other arrangements keep the
    /// replica's per-tab extents.
    pub fn grid_extents_with(&self, labels: &[f32], bounds: Rect) -> Vec<f32> {
        let raw = self.extents_with(labels, bounds);
        if !self.scroller_mode() || self.inner.size_mode != TabSizeMode::Normal {
            return raw;
        }
        let widest = raw.iter().copied().fold(0.0_f32, f32::max);
        vec![widest; raw.len()]
    }

    fn gap(&self) -> f32 {
        if self.scroller_mode() {
            TAB_GAP
        } else {
            0.0
        }
    }

    /// Each tab's `(left, right)` in CONTENT space — from the start of the
    /// unscrolled row, gaps included.
    fn spans(&self, extents: &[f32]) -> Vec<(f32, f32)> {
        let gap = self.gap();
        let mut x = 0.0;
        extents
            .iter()
            .map(|&w| {
                let s = (x, x + w);
                x += w + gap;
                s
            })
            .collect()
    }

    fn content_width(&self, extents: &[f32]) -> f32 {
        extents.iter().sum::<f32>() + self.gap() * extents.len().saturating_sub(1) as f32
    }

    fn along_extent(&self, bounds: Rect) -> f32 {
        if strip_is_vertical(self.inner.alignment) {
            bounds.bottom - bounds.top
        } else {
            bounds.right - bounds.left
        }
    }

    fn row_map(&self, extents: &[f32], bounds: Rect) -> Vec<u32> {
        if self.inner.multiline {
            tab_rows(extents, self.along_extent(bounds))
        } else {
            vec![0; extents.len()]
        }
    }

    /// The strip band along its page-facing edge, full width.
    fn band(&self, bounds: Rect, strip: f32) -> Rect {
        match self.inner.alignment {
            TabAlignment::Bottom => Rect::new(bounds.left, bounds.bottom - strip, bounds.right, bounds.bottom),
            _ => Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + strip),
        }
    }

    /// The scroll state of an overflowing underline strip; `None` when the
    /// strip is not a single horizontal row or when every tab fits.
    ///
    /// Pure. The arrows and the viewport depend on each other (an arrow that
    /// appears narrows the viewport, which moves the maximum) exactly as the
    /// web's flex row does; two passes settle it.
    pub fn scroll_with(&self, labels: &[f32], bounds: Rect) -> Option<TabScroll> {
        if !self.scroller_mode() {
            return None;
        }
        let extents = self.grid_extents_with(labels, bounds);
        let content = self.content_width(&extents);
        let width = bounds.right - bounds.left;
        if content <= width + TAB_SCROLL_SLACK {
            return None;
        }
        let strip = self.strip_thickness_with(labels, bounds);
        let band = self.band(bounds, strip);

        // (left arrow, right arrow, viewport width) for an offset.
        let arrows = |offset: f32| -> (bool, bool, f32) {
            let left = offset > TAB_SCROLL_SLACK;
            let mut vw = width - if left { TAB_ARROW_WIDTH } else { 0.0 };
            let right = offset < content - vw - TAB_SCROLL_SLACK;
            if right {
                vw -= TAB_ARROW_WIDTH;
            }
            (left, right, vw.max(0.0))
        };
        let clamp = |offset: f32, vw: f32| offset.clamp(0.0, (content - vw).max(0.0));

        let spans = self.spans(&extents);
        let selected = usize::try_from(self.inner.selected_index).ok().and_then(|i| spans.get(i)).copied();
        let mut offset = self.scroll.unwrap_or(0.0);
        for _ in 0..3 {
            let (_, _, vw) = arrows(offset);
            if self.scroll.is_none() {
                if let Some(span) = selected {
                    offset = reveal(offset, span, vw);
                }
            }
            offset = clamp(offset, vw);
        }
        let (left, right, vw) = arrows(offset);
        let max = (content - vw).max(0.0);
        let vl = band.left + if left { TAB_ARROW_WIDTH } else { 0.0 };
        Some(TabScroll {
            offset,
            max,
            viewport: Rect::new(vl, band.top, vl + vw, band.bottom),
            left_arrow: left.then(|| Rect::new(band.left, band.top, band.left + TAB_ARROW_WIDTH, band.bottom)),
            right_arrow: right
                .then(|| Rect::new(band.right - TAB_ARROW_WIDTH, band.top, band.right, band.bottom)),
        })
    }

    /// Every tab's rectangle inside `bounds`, from already-measured captions.
    /// Pure, so the wrapping, the selected-row rotation and the scroll are
    /// unit-testable. A scrolled strip's rectangles may lie outside its
    /// viewport — the paint clips them and [`Tabs::item_at`] ignores them.
    pub fn tab_rects_with(&self, labels: &[f32], bounds: Rect) -> Vec<Rect> {
        let extents = self.grid_extents_with(labels, bounds);
        let rows = self.row_map(&extents, bounds);
        let base = tab_rects(
            bounds,
            self.inner.alignment,
            &extents,
            self.row_thickness(),
            &rows,
            self.inner.size_mode,
            self.inner.selected_index,
        );
        if !self.scroller_mode() {
            return base;
        }
        // One row: the replica answers for the cross axis (the 2 DIP strip
        // inset, the page-facing edge); the along axis is the web grid's —
        // equal tracks, `gap-1`, shifted by the scroll.
        let (origin, offset) = match self.scroll_with(labels, bounds) {
            Some(s) => (s.viewport.left, s.offset),
            None => (bounds.left, 0.0),
        };
        base.iter()
            .zip(self.spans(&extents))
            .map(|(r, (l, rr))| Rect::new(origin + l - offset, r.top, origin + rr - offset, r.bottom))
            .collect()
    }

    fn labels(&self, c: &dyn Canvas) -> Vec<f32> {
        let f = c.formats();
        // `text-sm font-medium` on EVERY tab, active or not — the weight does
        // not change with the selection, so neither does the width.
        self.inner.tab_pages.iter().map(|p| text_width(c, &p.control().text, &f.body_strong)).collect()
    }

    /// Every tab's rectangle inside `bounds`.
    pub fn tab_rectangles(&self, c: &dyn Canvas, bounds: Rect) -> Vec<Rect> {
        self.tab_rects_with(&self.labels(c), bounds)
    }

    /// The scroll state inside `bounds` (see [`Tabs::scroll_with`]).
    pub fn scroll_state(&self, c: &dyn Canvas, bounds: Rect) -> Option<TabScroll> {
        self.scroll_with(&self.labels(c), bounds)
    }

    /// Tab `i`'s CONTENT-space span — what [`TabsPaint::indicator`] animates
    /// between.
    pub fn content_span(&self, c: &dyn Canvas, bounds: Rect, i: usize) -> Option<(f32, f32)> {
        let labels = self.labels(c);
        self.spans(&self.grid_extents_with(&labels, bounds)).get(i).copied()
    }

    /// The offset one click on a scroll arrow leads to: `scrollByPage`,
    /// `Math.max(120, clientWidth * 0.75)` in direction `dir` (-1 / 1),
    /// clamped. 0 when the strip does not scroll.
    pub fn scrolled_by_page(&self, c: &dyn Canvas, bounds: Rect, dir: i32) -> f32 {
        let Some(s) = self.scroll_state(c, bounds) else { return 0.0 };
        let vw = s.viewport.right - s.viewport.left;
        let step = TAB_PAGE_MIN.max(vw * TAB_PAGE_FRACTION);
        (s.offset + dir.signum() as f32 * step).clamp(0.0, s.max)
    }

    /// The offset that brings the selected tab into view from the current
    /// one — what a stateful caller stores in [`Tabs::scroll`] after the
    /// selection changed (`useEffect(() => revealActive(true), [value])`).
    pub fn reveal_offset(&self, c: &dyn Canvas, bounds: Rect) -> f32 {
        let labels = self.labels(c);
        let spans = self.spans(&self.grid_extents_with(&labels, bounds));
        let span = usize::try_from(self.inner.selected_index).ok().and_then(|i| spans.get(i)).copied();
        let mut offset = self.scroll.unwrap_or(0.0);
        // The viewport narrows or widens as the arrows come and go; settle it.
        for _ in 0..3 {
            let at = Tabs { scroll: Some(offset), ..self.clone() };
            let Some(s) = at.scroll_with(&labels, bounds) else { return 0.0 };
            let vw = s.viewport.right - s.viewport.left;
            offset = match span {
                Some(sp) => reveal(s.offset, sp, vw),
                None => s.offset,
            }
            .clamp(0.0, s.max);
        }
        offset
    }

    /// The scroll arrow under `(x, y)`: `-1` left, `1` right.
    pub fn arrow_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<i32> {
        let s = self.scroll_state(c, bounds)?;
        if s.left_arrow.is_some_and(|r| r.contains(x, y)) {
            Some(-1)
        } else if s.right_arrow.is_some_and(|r| r.contains(x, y)) {
            Some(1)
        } else {
            None
        }
    }

    /// Whether page `i` can be selected (enabled, and there).
    pub fn is_selectable(&self, i: usize) -> bool {
        self.inner.tab_pages.get(i).is_some_and(|p| p.control().enabled)
    }

    /// The tab the arrows / Home / End select from the current one — the
    /// ARIA tab list with automatic activation, wrapping at both ends.
    pub fn step_selection(&self, key: NavKey) -> Option<usize> {
        let from = usize::try_from(self.inner.selected_index).ok();
        roving_step(self.inner.tab_pages.len(), from, key, true, |i| self.is_selectable(i))
    }

    /// The whole strip band's thickness, rows and 2 DIP insets included.
    pub fn strip_thickness_with(&self, labels: &[f32], bounds: Rect) -> f32 {
        let extents = self.extents_with(labels, bounds);
        let rows = self.row_map(&extents, bounds);
        let n = rows.iter().copied().max().map(|r| r + 1).unwrap_or(1);
        tab_strip_thickness(self.row_thickness(), n)
    }

    /// The rectangle the selected page is given — the replica's
    /// `DisplayRectangle`, taken against the `bounds` argument rather than the
    /// model's own rectangle.
    pub fn page_rect(&self, c: &dyn Canvas, bounds: Rect) -> Rect {
        let strip = self.strip_thickness_with(&self.labels(c), bounds);
        tab_display_rect(bounds, self.inner.alignment, strip)
    }

    /// The tab under `(x, y)`. In a scrolled strip only the part inside the
    /// viewport counts — a tab hidden under an arrow is not under the pointer.
    pub fn item_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        let labels = self.labels(c);
        if let Some(s) = self.scroll_with(&labels, bounds) {
            if !s.viewport.contains(x, y) {
                return None;
            }
        }
        self.tab_rects_with(&labels, bounds).into_iter().position(|r| r.contains(x, y))
    }

    /// The strip's hairline rule, on the edge facing the page.
    fn rule_rect(&self, bounds: Rect, strip: f32) -> Rect {
        match self.inner.alignment {
            TabAlignment::Top => {
                Rect::new(bounds.left, bounds.top + strip - 1.0, bounds.right, bounds.top + strip)
            }
            TabAlignment::Bottom => Rect::new(
                bounds.left,
                bounds.bottom - strip,
                bounds.right,
                bounds.bottom - strip + 1.0,
            ),
            TabAlignment::Left => {
                Rect::new(bounds.left + strip - 1.0, bounds.top, bounds.left + strip, bounds.bottom)
            }
            TabAlignment::Right => Rect::new(
                bounds.right - strip,
                bounds.top,
                bounds.right - strip + 1.0,
                bounds.bottom,
            ),
        }
    }

    /// One tab's indicator band and its hover fill, both on the page-facing
    /// edge. The band is inset by [`control::TAB_UNDERLINE_INSET`] (`INSET =
    /// 8`) — it deliberately does **not** run wall to wall — and the hover
    /// stops one DIP short of the rule so the two never overlap.
    /// The indicator band and the hover wash of a tab whose rectangle already
    /// reaches the rule (see [`Tabs::reach_rule`]): the wash is the whole tab,
    /// the indicator its page-facing `TAB_UNDERLINE` band.
    fn tab_edges(&self, r: Rect) -> (Rect, Rect) {
        let u = control::TAB_UNDERLINE;
        let m = control::TAB_UNDERLINE_INSET;
        let band = match self.inner.alignment {
            TabAlignment::Top => Rect::new(r.left + m, r.bottom - u, r.right - m, r.bottom),
            TabAlignment::Bottom => Rect::new(r.left + m, r.top, r.right - m, r.top + u),
            TabAlignment::Left => Rect::new(r.right - u, r.top + m, r.right, r.bottom - m),
            TabAlignment::Right => Rect::new(r.left, r.top + m, r.left + u, r.bottom - m),
        };
        (band, r)
    }

    /// A tab's rectangle with its page-facing edge moved onto the rule's near
    /// edge. The replica insets its tabs inside the strip; the web's tab fills
    /// the strip down to the container's `border-b`, and its indicator is
    /// `absolute bottom-0` — so the hover wash and the blue bar sit ON the grey
    /// rule, touching it, never floating a few pixels above it.
    fn reach_rule(&self, r: Rect, rule: Rect) -> Rect {
        match self.inner.alignment {
            TabAlignment::Top => Rect::new(r.left, r.top, r.right, rule.top),
            TabAlignment::Bottom => Rect::new(r.left, rule.bottom, r.right, r.bottom),
            TabAlignment::Left => Rect::new(r.left, r.top, rule.left, r.bottom),
            TabAlignment::Right => Rect::new(rule.right, r.top, r.right, r.bottom),
        }
    }

    /// Paints the strip with one tab drawn hot.
    pub fn paint_tabs(&self, c: &dyn Canvas, bounds: Rect, hot: Option<usize>) {
        self.paint_with(c, bounds, &TabsPaint { hot, ..TabsPaint::default() });
    }

    /// Paints the strip with its full transient state: hover, the keyboard
    /// ring, the scroll arrows of an overflowing strip and the (possibly
    /// travelling) indicator.
    pub fn paint_with(&self, c: &dyn Canvas, bounds: Rect, s: &TabsPaint) {
        let t = c.theme();
        let f = c.formats();
        let labels = self.labels(c);
        let strip = self.strip_thickness_with(&labels, bounds);
        let scroll = self.scroll_with(&labels, bounds);

        // The `border-b border-border` on the OUTER box, so the rule still
        // spans the full width once the arrows and the scroller split it.
        let rule = self.rule_rect(bounds, strip);
        c.fill_rounded(&rule, 0.0, &t.divider);

        if let Some(sc) = scroll {
            // `arrowCls`: `text-text-secondary hover:text-text-primary`, no fill.
            for (arrow, dir, name) in [(sc.left_arrow, -1, "ChevronLeft"), (sc.right_arrow, 1, "ChevronRight")] {
                if let Some(r) = arrow {
                    let ink = if s.hot_arrow == Some(dir) { t.text_primary } else { t.text_secondary };
                    let glyph = Rect::new(r.left, r.top, r.right, r.bottom - 1.0);
                    c.vector_icon(name, &glyph, TAB_ARROW_ICON, &ink);
                }
            }
        }

        let clip = scroll.map(|sc| sc.viewport).unwrap_or(bounds);
        c.push_clip(&clip);
        let rects = self.tab_rects_with(&labels, bounds);
        // `DrawMode.OwnerDrawFixed`: the owner draws each tab's content (the strip keeps its
        // hover wash, rule and indicator).
        let owner = (self.inner.draw_mode == kubuno_controls::layout_panels::TabDrawMode::OwnerDrawFixed
            && crate::graphics::owner_draw::has_handler())
        .then(|| crate::graphics::Graphics::new(c));
        for (i, rect) in rects.iter().copied().enumerate() {
            let Some(page) = self.inner.tab_pages.get(i) else { continue };
            if rect.right < clip.left || rect.left > clip.right {
                continue;
            }
            let active = i as i32 == self.inner.selected_index;
            let enabled = page.control().enabled;
            let (_, hover) = self.tab_edges(self.reach_rule(rect, rule));
            let hot = enabled && (s.hot == Some(i) || s.pressed == Some(i));

            // `.kb-tab:hover` tints the whole tab, active or not: the other
            // tabs in `surface-2`, the active one in its OWN blue (`#eaeffa`,
            // the `primary-light` family) — « a grey wash under blue text
            // reads as the tab going quiet ».
            if hot {
                let wash = if active { t.accent_light } else { t.surface_2 };
                c.fill_rounded(&hover, 0.0, &wash);
            }

            // `text-primary` when active; `text-text-secondary
            // hover:text-text-primary` otherwise. Every tab is `font-medium`.
            let ink = if active {
                t.accent
            } else if hot {
                t.text_primary
            } else {
                t.text_secondary
            };
            let ink = if enabled { ink } else { dimmed(&ink) };
            // A tab wider than the viewport would have its caption cut
            // mid-glyph by the clip: centre and ellipsize it within the part
            // that shows instead.
            let text_box = if rect.right - rect.left > clip.right - clip.left {
                Rect::new(rect.left.max(clip.left), rect.top, rect.right.min(clip.right), rect.bottom)
            } else {
                rect
            };
            let mut drawn = false;
            if let Some(g) = &owner {
                use crate::graphics::owner_draw::{DrawItemEventArgs, DrawItemState};
                let st = DrawItemState::NONE
                    .with(DrawItemState::SELECTED, active)
                    .with(DrawItemState::HOT_LIGHT, hot)
                    .with(DrawItemState::FOCUS, s.focused == Some(i))
                    .with(DrawItemState::DISABLED, !enabled);
                let mut e = DrawItemEventArgs::new(g, "Tabs", Some(i), text_box, st, page.control().text.as_str());
                e.fore_color = ink.into();
                drawn = crate::graphics::owner_draw::draw_item(&mut e);
            }
            if !drawn {
                c.text_ellipsis_center(&page.control().text, &text_box, &f.body_strong, &ink);
            }

            if s.focus_visible && s.focused == Some(i) {
                paint_ring(c, hover, radius::SM);
            }

            if active && (s.indicator.is_none() || !self.scroller_mode()) {
                self.paint_indicator(c, self.reach_rule(rect, rule));
            }
        }
        // The travelling indicator: ONE mark for the whole strip, placed from
        // a content-space span so it can slide between two tabs.
        if let (Some((l, r)), true) = (s.indicator, self.scroller_mode()) {
            let (origin, offset) = match scroll {
                Some(sc) => (sc.viewport.left, sc.offset),
                None => (bounds.left, 0.0),
            };
            if let Some(base) = rects.first() {
                let rect = Rect::new(origin + l - offset, base.top, origin + r - offset, base.bottom);
                self.paint_indicator(c, self.reach_rule(rect, rule));
            }
        }
        c.pop_clip();
    }

    fn paint_indicator(&self, c: &dyn Canvas, rect: Rect) {
        let t = c.theme();
        let (indicator, _) = self.tab_edges(rect);
        // `rounded-t-[3px]` — only a top-aligned strip has a « top » for that
        // radius to mean anything; the other three draw the same band square.
        if self.inner.alignment == TabAlignment::Top {
            c.fill_top_rounded(&indicator, control::TAB_UNDERLINE, &t.accent);
        } else {
            c.fill_rounded(&indicator, 0.0, &t.accent);
        }
    }
}

/// The scroll offset that brings `span` (content space) into a viewport of
/// width `vw` currently at `offset` — `revealActive`'s arithmetic: scroll
/// back to the tab's left edge, or forward until its right edge shows, and
/// otherwise stay put.
fn reveal(offset: f32, span: (f32, f32), vw: f32) -> f32 {
    // A tab wider than the viewport shows its START, where its label begins:
    // `Tabs.tsx` would align its end (`right - clientWidth`) and cut the head
    // of the caption — the CSSOM `nearest` rule aligns the leading edge of an
    // element larger than its scroller, and so do we.
    if span.1 - span.0 > vw {
        return span.0;
    }
    if span.0 < offset {
        span.0
    } else if span.1 > offset + vw {
        span.1 - vw
    } else {
        offset
    }
}

impl Deref for Tabs {
    type Target = TabControl;
    fn deref(&self) -> &TabControl {
        &self.inner
    }
}
impl DerefMut for Tabs {
    fn deref_mut(&mut self) -> &mut TabControl {
        &mut self.inner
    }
}

impl Widget for Tabs {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, c: &dyn Canvas) -> Size {
        let labels = self.labels(c);
        // Measured against an unbounded strip: what the tabs want, not what
        // they were given.
        let wide = Rect::new(0.0, 0.0, f32::MAX / 4.0, f32::MAX / 4.0);
        let along = self.content_width(&self.grid_extents_with(&labels, wide));
        let across = tab_strip_thickness(self.row_thickness(), 1);
        if strip_is_vertical(self.inner.alignment) {
            Size::new(across, along)
        } else {
            Size::new(along, across)
        }
    }

    fn paint(&self, c: &dyn Canvas, bounds: Rect, _state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        c.fill_rounded(&bounds, 0.0, &c.current_bg());
        self.paint_tabs(c, bounds, None);
    }

    fn type_name(&self) -> &'static str {
        "Tabs"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// StatusBar
// ═════════════════════════════════════════════════════════════════════════════

/// The Kubuno status bar.
///
/// Owns a [`StatusStrip`], which is a `ToolStrip` that already docks to the
/// bottom, hides the grip, disables overflow and lays its items out with
/// `Table` — the layout style whose whole purpose is `Spring`, the « share the
/// leftover width » flag a status label carries. That sharing is
/// [`layout_items`]' own arithmetic (whole-unit share plus a remainder handed
/// out one cell at a time), so this primitive does none of it.
///
/// The paint is the shipping bar's: no background of its own, 12 px meta text
/// in `text_secondary` at `space::SM` from the edge, and — for the cells that
/// are buttons — a `radius::SM` hover pill with a 16 DIP icon, exactly as
/// `drive-app`'s git widgets draw.
#[derive(Clone)]
pub struct StatusBar {
    inner: StatusStrip,
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl StatusBar {
    pub fn new() -> Self {
        Self { inner: StatusStrip::new() }
    }

    /// Appends a cell.
    pub fn with(mut self, item: StripItem) -> Self {
        self.inner.items.push(item);
        self
    }

    /// The width one cell wants.
    fn item_width(&self, c: &dyn Canvas, item: &StripItem) -> f32 {
        let f = c.formats();
        match item {
            StripItem::Separator(_) => SEPARATOR_WIDTH,
            // Measured with the very format the paint uses, ceiled, plus the
            // cell's own insets — so a cell that has room never truncates.
            StripItem::Button(_) => {
                STATUS_LABEL_LEFT + text_width(c, &item.item().text, &f.caption) + STATUS_TEXT_INSET
            }
            _ => text_width(c, &item.item().text, &f.caption) + 2.0 * STATUS_TEXT_INSET,
        }
    }

    /// Whether cell `i` can take the keyboard focus — the button cells (the
    /// git widgets); text cells are information, not controls.
    pub fn is_focusable(&self, i: usize) -> bool {
        self.inner.items.get(i).is_some_and(|it| {
            it.item().visible && it.item().enabled && matches!(it, StripItem::Button(_))
        })
    }

    /// Every cell's desired width, in collection order.
    pub fn item_widths(&self, c: &dyn Canvas) -> Vec<f32> {
        self.inner.items.iter().map(|it| self.item_width(c, it)).collect()
    }

    /// Places already-measured cells into `bounds`. Pure: it is one call to the
    /// replica's [`layout_items`], springing enabled, which is what `Table`
    /// resolves to.
    ///
    /// Only the `Spring` cells give: they absorb the leftover width when there
    /// is some, and when the bar is too narrow they SHRINK first (down to
    /// nothing), so the fixed cells — counts, widgets — keep their full text
    /// for as long as possible.
    pub fn item_rects_with(&self, widths: &[f32], bounds: Rect) -> StripLayout {
        let widths = shrink_springs(
            widths,
            &self
                .inner
                .items
                .iter()
                .map(|it| (it.item().visible, matches!(it, StripItem::StatusLabel(s) if s.spring)))
                .collect::<Vec<_>>(),
            bounds.right - bounds.left,
        );
        let inputs: Vec<StripItemInput> = self
            .inner
            .items
            .iter()
            .enumerate()
            .map(|(i, it)| StripItemInput {
                size: Size::new(widths.get(i).copied().unwrap_or(0.0), bounds.bottom - bounds.top),
                alignment: it.item().alignment,
                overflow: it.item().overflow,
                spring: matches!(it, StripItem::StatusLabel(s) if s.spring),
                visible: it.item().visible,
            })
            .collect();
        layout_items(StripAxis::Horizontal, bounds, 0.0, 0.0, false, true, &inputs)
    }

    /// Every cell's rectangle inside `bounds`.
    pub fn item_rects(&self, c: &dyn Canvas, bounds: Rect) -> StripLayout {
        self.item_rects_with(&self.item_widths(c), bounds)
    }

    /// The cell under `(x, y)`.
    pub fn item_at(&self, c: &dyn Canvas, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        self.item_rects(c, bounds)
            .rects
            .into_iter()
            .enumerate()
            .find(|(i, r)| {
                self.inner.items[*i].item().visible && r.right > r.left && r.contains(x, y)
            })
            .map(|(i, _)| i)
    }

    /// Paints the bar with one cell drawn hot.
    pub fn paint_items(&self, c: &dyn Canvas, bounds: Rect, hot: Option<usize>) {
        self.paint_with(c, bounds, &StripPaint::hot(hot));
    }

    /// Paints the bar with its full transient state: hover and press on the
    /// button cells, and the keyboard ring around the focused one.
    pub fn paint_with(&self, c: &dyn Canvas, bounds: Rect, s: &StripPaint) {
        let t = c.theme();
        let f = c.formats();
        let layout = self.item_rects(c, bounds);
        let hot = s.hot;
        // Cells that still do not fit once the springs are gone are clipped at
        // the bar's edge rather than painted over its neighbour.
        c.push_clip(&bounds);

        for (i, item) in self.inner.items.iter().enumerate() {
            let rect = layout.rects[i];
            if !item.item().visible || rect.right <= rect.left {
                continue;
            }
            let it = item.item();
            // A cell's own `ForeColor` (a warning in Caution) wins over the bar's secondary ink.
            let base = it.fore_color.unwrap_or(t.text_secondary);
            let ink = if it.enabled { base } else { dimmed(&base) };

            match item {
                StripItem::Separator(_) => {
                    let mid = (rect.left + rect.right) / 2.0;
                    let bar = Rect::new(
                        mid,
                        rect.top + SEPARATOR_INSET,
                        mid + 1.0,
                        rect.bottom - SEPARATOR_INSET,
                    );
                    c.fill_rounded(&bar, 0.0, &t.divider);
                }
                StripItem::Button(_) => {
                    // `@ui/Button` variant "ghost": no fill and no outline at
                    // rest, `hover:bg-surface-2`, radius `rounded-md`.
                    let pill_rect = rect.inflate(-STATUS_HOVER_INSET.0, -STATUS_HOVER_INSET.1);
                    if s.pressed == Some(i) && it.enabled {
                        // `active:` on the ghost button: one step darker.
                        c.fill_rounded(&pill_rect, radius::SM, &t.surface_3);
                    } else if hot == Some(i) && it.enabled {
                        c.fill_rounded(&pill_rect, radius::SM, &t.surface_2);
                    }
                    if s.ring(i) && it.enabled {
                        paint_ring(c, pill_rect, radius::SM);
                    }
                    if let Some(name) = item_icon(item) {
                        let cy = (rect.top + rect.bottom) / 2.0;
                        let half = SIDEBAR_ICON_GLYPH / 2.0;
                        let icon = Rect::new(
                            rect.left + STATUS_ICON_LEFT,
                            cy - half,
                            rect.left + STATUS_ICON_RIGHT,
                            cy + half,
                        );
                        c.vector_icon(name, &icon, SIDEBAR_ICON_GLYPH, &ink);
                    }
                    let label = Rect::new(
                        rect.left + STATUS_LABEL_LEFT,
                        rect.top,
                        rect.right - STATUS_TEXT_INSET,
                        rect.bottom,
                    );
                    c.text_ellipsis(&it.text, &label, &f.caption, &ink);
                }
                _ => {
                    let label = Rect::new(
                        rect.left + STATUS_TEXT_INSET,
                        rect.top,
                        rect.right - STATUS_TEXT_INSET,
                        rect.bottom,
                    );
                    c.text_ellipsis(&it.text, &label, &f.caption, &ink);
                }
            }
        }
        c.pop_clip();
    }
}

/// `widths` with the `Spring` cells shrunk, evenly and down to zero, by as
/// much as the visible cells overrun `avail`. `cells` is `(visible, spring)`
/// per cell. Nothing changes when everything fits.
fn shrink_springs(widths: &[f32], cells: &[(bool, bool)], avail: f32) -> Vec<f32> {
    let mut out = widths.to_vec();
    let total: f32 = cells
        .iter()
        .enumerate()
        .filter(|(_, (visible, _))| *visible)
        .map(|(i, _)| widths.get(i).copied().unwrap_or(0.0))
        .sum();
    let mut deficit = total - avail;
    // Share the deficit out; a spring that hits zero hands its unpaid share
    // to the others on the next pass.
    for _ in 0..cells.len() {
        if deficit <= 0.0 {
            break;
        }
        let live: Vec<usize> = (0..cells.len())
            .filter(|&i| cells[i].0 && cells[i].1 && out.get(i).copied().unwrap_or(0.0) > 0.0)
            .collect();
        if live.is_empty() {
            break;
        }
        let share = deficit / live.len() as f32;
        for i in live {
            let cut = share.min(out[i]);
            out[i] -= cut;
            deficit -= cut;
        }
    }
    out
}

impl Deref for StatusBar {
    type Target = StatusStrip;
    fn deref(&self) -> &StatusStrip {
        &self.inner
    }
}
impl DerefMut for StatusBar {
    fn deref_mut(&mut self) -> &mut StatusStrip {
        &mut self.inner
    }
}

impl Widget for StatusBar {
    fn model(&self) -> &dyn Control {
        &self.inner
    }

    fn measure(&self, c: &dyn Canvas) -> Size {
        // `STATUSBAR_HEIGHT` in the shipping app is 32, which is the design
        // system's `height::BUTTON_SM`.
        Size::new(self.item_widths(c).iter().sum(), height::BUTTON_SM)
    }

    fn paint(&self, c: &dyn Canvas, bounds: Rect, _state: WidgetState) {
        // Rule: every widget lands on an opaque background. What follows
        // may overpaint most of it (a fill, a card, a track); this makes
        // sure the parts that stay uncovered still read as an opaque
        // surface, so a widget never shows what is behind it.
        c.fill_rounded(&bounds, 0.0, &c.current_bg());
        self.paint_items(c, bounds, None);
    }

    fn type_name(&self) -> &'static str {
        "StatusBar"
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Tests — the geometry, without a window
//
// Every one of these is canvas-free: the primitives split measurement (which
// needs DirectWrite) from arrangement (which does not), precisely so the part
// that can regress silently can be pinned here.
// ═════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    // Pulled in here rather than at the top of the file: they exist to state
    // what the tests compare against, and the paint bodies never touch them.
    use drive_app_controls::sidebar::INDENT_PER_LEVEL;
    use drive_app_controls::toolbar::OVERFLOW_BUTTON_WIDTH;
    use kubuno_controls::toolstrip::ToolStripItemAlignment;

    const EPS: f32 = 0.001;

    fn same(a: Rect, b: Rect) -> bool {
        (a.left - b.left).abs() < EPS
            && (a.top - b.top).abs() < EPS
            && (a.right - b.right).abs() < EPS
            && (a.bottom - b.bottom).abs() < EPS
    }

    fn bar(n: usize) -> Toolbar {
        let mut t = Toolbar::new();
        for _ in 0..n {
            t.items.push(icon_item("Copy"));
        }
        t
    }

    fn widths(t: &Toolbar, w: f32) -> Vec<(usize, f32)> {
        (0..t.items.len()).map(|i| (i, w)).collect()
    }

    // ── Toolbar ─────────────────────────────────────────────────────────────

    #[test]
    fn toolbar_geometry_equals_the_predecessor() {
        // The gate: for the same measured widths, the rebuilt bar must place
        // its items exactly where `drive_app_controls::toolbar::arrange` does.
        let t = bar(4);
        let bounds = Rect::new(20.0, 8.0, 420.0, 44.0);
        let got = t.arrange_widths(&widths(&t, 40.0), bounds);

        let measures = [ItemMeasure { width: 40.0, overflow: OverflowBehavior::Auto }; 4];
        let want = arrange(&measures, 400.0, 8.0, height::BUTTON_MD, ITEM_SPACING);

        assert_eq!(got.visible.len(), want.visible.len());
        for ((gi, gr), (wi, wr)) in got.visible.iter().zip(want.visible.iter()) {
            assert_eq!(gi, wi);
            assert!(same(*gr, wr.shift_x(bounds.left)), "item {gi} moved");
        }
        assert!(got.overflow_button.is_none() && want.overflow_button.is_none());
    }

    #[test]
    fn toolbar_overflows_from_the_tail_and_reserves_the_button() {
        let t = bar(6);
        // Six 40 DIP items plus five 4 DIP gaps want 260; give them 160.
        let bounds = Rect::new(0.0, 0.0, 160.0, height::BUTTON_MD);
        let a = t.arrange_widths(&widths(&t, 40.0), bounds);

        assert!(!a.overflow.is_empty(), "something must overflow");
        assert!(a.overflow_button.is_some(), "and the More button must appear");
        // The tail goes first: the overflowed indices are the last ones.
        let first_over = *a.overflow.iter().min().expect("non-empty");
        assert!(a.visible.iter().all(|(i, _)| *i < first_over));
        // What stays fits inside the budget the button leaves.
        let btn = a.overflow_button.expect("checked above");
        let last = a.visible.last().expect("at least one stays").1;
        assert!(last.right <= btn.left + EPS);
        assert!((btn.right - btn.left - OVERFLOW_BUTTON_WIDTH).abs() < EPS);
    }

    #[test]
    fn toolbar_honours_the_replica_overflow_rule() {
        let mut t = Toolbar::new();
        let mut never = icon_item("Cut");
        let mut always = icon_item("Copy");
        match &mut never {
            StripItem::Button(b) => b.item.overflow = ToolStripItemOverflow::Never,
            _ => unreachable!(),
        }
        match &mut always {
            StripItem::Button(b) => b.item.overflow = ToolStripItemOverflow::Always,
            _ => unreachable!(),
        }
        t.items.push(never);
        t.items.push(always);

        let a = t.arrange_widths(&widths(&t, 40.0), Rect::new(0.0, 0.0, 500.0, 36.0));
        assert!(a.overflow.contains(&1), "Always always overflows");
        assert!(a.visible.iter().any(|(i, _)| *i == 0), "Never never does");
    }

    #[test]
    fn toolbar_item_at_is_half_open() {
        let t = bar(2);
        let bounds = Rect::new(0.0, 0.0, 400.0, 36.0);
        let a = t.arrange_widths(&widths(&t, 40.0), bounds);
        let first = a.visible[0].1;
        let second = a.visible[1].1;
        // Left edge belongs to the item, right edge does not — `Rect::contains`.
        assert!(first.contains(first.left, first.top));
        assert!(!first.contains(first.right, first.top));
        assert!(!first.contains(first.left, first.bottom));
        // The gap between two items belongs to neither.
        let gap = (first.right + second.left) / 2.0;
        assert!(!first.contains(gap, first.top) && !second.contains(gap, first.top));
    }

    #[test]
    fn toolbar_skips_invisible_items() {
        let mut t = bar(3);
        match &mut t.items[1] {
            StripItem::Button(b) => b.item.visible = false,
            _ => unreachable!(),
        }
        let w: Vec<(usize, f32)> = vec![(0, 40.0), (2, 40.0)];
        let a = t.arrange_widths(&w, Rect::new(0.0, 0.0, 400.0, 36.0));
        assert_eq!(a.visible.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![0, 2]);
    }

    // ── Sidebar ─────────────────────────────────────────────────────────────

    fn pane() -> Sidebar {
        Sidebar::new()
            .with(nav_item("Home", "Accueil", true, 28.0), None)
            .with(label_item("Épinglés"), Some(true))
            .with(nav_item("Star", "Documents", false, 48.0), None)
    }

    #[test]
    fn sidebar_rows_match_the_shipping_geometry() {
        // The shipping pane lays rows at `left + 8 … right - 8`, 36 tall, two
        // DIP apart, with 12 above a section header.
        let s = pane();
        let bounds = Rect::new(0.0, 100.0, 260.0, 700.0);
        let r = s.row_rects(bounds);

        for row in &r {
            assert!((row.left - (bounds.left + space::SM)).abs() < EPS);
            assert!((row.right - (bounds.right - space::SM)).abs() < EPS);
            assert!((row.bottom - row.top - SIDEBAR_ROW_HEIGHT).abs() < EPS);
        }
        assert!((r[0].top - bounds.top).abs() < EPS);
        // Row 1 is a section: 2 DIP of row gap, then the 12 DIP section gap.
        assert!((r[1].top - (r[0].bottom + space::XXS + SIDEBAR_SECTION_GAP)).abs() < EPS);
        // Row 2 is an ordinary row: just the 2 DIP gap.
        assert!((r[2].top - (r[1].bottom + space::XXS)).abs() < EPS);
    }

    #[test]
    fn sidebar_row_width_matches_the_predecessor() {
        // The natural width is the predecessor's own sum, term for term.
        let indent = 48.0;
        let text = 120.0;
        let want = drive_app_controls::sidebar::sidebar_row_width(indent, text);
        assert!((want - (space::SM + indent + ROW_ICON_SIZE + ROW_ICON_TEXT_GAP + text
            + ROW_TEXT_RIGHT_MARGIN))
            .abs()
            < EPS);
        // And the indent a level costs is the predecessor's constant.
        assert!((INDENT_PER_LEVEL - 16.0).abs() < EPS);
    }

    #[test]
    fn sidebar_collapses_to_the_compact_rail() {
        let mut s = pane();
        s.mode = SidebarMode::Compact;
        assert!(
            (s.pane_width() - drive_app_controls::sidebar::SIDEBAR_COMPACT_WIDTH).abs() < EPS
        );

        // The rail's rows are the pane less the two 8 DIP insets — the 40 × 36
        // stadium the predecessor documents.
        let bounds = Rect::new(0.0, 0.0, s.pane_width(), 400.0);
        let r = s.row_rects(bounds);
        assert!((r[0].right - r[0].left - 40.0).abs() < EPS);
        assert!((r[0].bottom - r[0].top - SIDEBAR_ROW_HEIGHT).abs() < EPS);

        // Minimal is the floating 300 DIP pane; Expanded clamps the stored
        // preference — both are `sidebar_pane_width`, unchanged.
        s.mode = SidebarMode::Minimal;
        assert!(
            (s.pane_width() - drive_app_controls::sidebar::SIDEBAR_OPEN_PANE_LENGTH).abs() < EPS
        );
        s.mode = SidebarMode::Expanded;
        s.control_mut().bounds = Rect::new(0.0, 0.0, 50.0, 400.0);
        assert!((s.pane_width() - drive_app_controls::sidebar::SIDEBAR_MIN_WIDTH).abs() < EPS);
    }

    #[test]
    fn sidebar_item_at_is_half_open_and_skips_the_gaps() {
        let s = pane();
        let bounds = Rect::new(0.0, 0.0, 260.0, 700.0);
        let r = s.row_rects(bounds);

        assert_eq!(s.item_at(bounds, r[0].left, r[0].top), Some(0));
        assert_eq!(s.item_at(bounds, r[0].right - 0.5, r[0].bottom - 0.5), Some(0));
        // The bottom edge belongs to the next row's gap, not to this row.
        assert_eq!(s.item_at(bounds, r[0].left, r[0].bottom), None);
        // Outside the 8 DIP inset there is no row at all.
        assert_eq!(s.item_at(bounds, bounds.left + 1.0, r[0].top), None);
        assert_eq!(s.item_at(bounds, r[2].left, r[2].top), Some(2));
    }

    #[test]
    fn sidebar_active_row_is_the_replica_checked_flag() {
        let s = pane();
        assert!(matches!(&s.items[0], StripItem::Button(b) if b.checked));
        assert!(matches!(&s.items[2], StripItem::Button(b) if !b.checked));
        // And the indent lives in the replica's own padding.
        assert!((s.items[2].item().padding.left - 48.0).abs() < EPS);
    }

    // ── Breadcrumb ──────────────────────────────────────────────────────────

    fn trail() -> Breadcrumb {
        Breadcrumb::new().with("Ce PC").with("Documents").with("Projets").with("Kubuno")
    }

    #[test]
    fn breadcrumb_matches_layout_breadcrumbs_when_everything_fits() {
        let b = trail();
        let w = [80.0, 100.0, 90.0, 110.0];
        let bounds = Rect::new(10.0, 4.0, 900.0, 36.0);
        let got = b.layout_with(&w, bounds);

        let want = layout_breadcrumbs(
            &w,
            &BreadcrumbLayoutParams {
                start_x: bounds.left,
                avail_right: bounds.right,
                top: bounds.top,
                bottom: bounds.bottom,
                chevron_block: CHEVRON_BLOCK,
                ellipsis_width: ELLIPSIS_WIDTH,
            },
        );
        assert_eq!(got.items.len(), want.items.len());
        assert_eq!(got.start_index, want.start_index);
        for (g, wa) in got.items.iter().zip(want.items.iter()) {
            assert!(same(*g, *wa));
        }
        assert!(got.ellipsis.is_none() && want.ellipsis.is_none());
    }

    #[test]
    fn breadcrumb_ellipsis_is_the_predecessor_ellipsis() {
        let b = trail();
        let w = [80.0, 100.0, 90.0, 110.0];
        // Far too narrow: the head must fold.
        let bounds = Rect::new(0.0, 0.0, 200.0, 32.0);
        let got = b.layout_with(&w, bounds);

        let want = layout_breadcrumbs(
            &w,
            &BreadcrumbLayoutParams {
                start_x: 0.0,
                avail_right: 200.0,
                top: 0.0,
                bottom: 32.0,
                chevron_block: CHEVRON_BLOCK,
                ellipsis_width: ELLIPSIS_WIDTH,
            },
        );
        assert!(got.ellipsis.is_some(), "the head must fold");
        assert!(same(
            got.ellipsis.expect("checked"),
            want.ellipsis.expect("same inputs")
        ));
        assert_eq!(got.start_index, want.start_index);
        // The TAIL is what survives: the current folder is always drawn.
        assert_eq!(got.start_index + got.items.len(), w.len());
        for (g, wa) in got.items.iter().zip(want.items.iter()) {
            assert!(same(*g, *wa));
        }
    }

    #[test]
    fn breadcrumb_root_chevron_costs_one_block() {
        let mut b = trail();
        let w = [40.0, 40.0];
        let bounds = Rect::new(0.0, 0.0, 600.0, 32.0);
        let without = b.layout_with(&w, bounds);
        b.root_chevron = true;
        let with = b.layout_with(&w, bounds);
        assert!((with.items[0].left - without.items[0].left - CHEVRON_BLOCK).abs() < EPS);
        let r = b.root_chevron_rect(bounds).expect("root chevron is on");
        assert!((r.left - CHEVRON_OFFSET).abs() < EPS);
        assert!((r.right - r.left - CHEVRON_SIZE).abs() < EPS);
    }

    // ── Tabs ────────────────────────────────────────────────────────────────

    fn strip() -> Tabs {
        let mut t = Tabs::new().with("Détails").with("Aperçu").with("Historique");
        t.selected_index = 1;
        t
    }

    #[test]
    fn tabs_take_their_extents_from_the_replica() {
        let t = strip();
        let labels = [60.0, 50.0, 80.0];
        let bounds = Rect::new(0.0, 0.0, 600.0, 200.0);
        let got = t.extents_with(&labels, bounds);
        let want: Vec<f32> = tab_strip(&labels, space::LG, 0.0, TabSizeMode::Normal, 600.0)
            .iter()
            .map(|p| p.1)
            .collect();
        assert_eq!(got, want);
        // `px-4` on each side: 60 + 2 * 16.
        assert!((got[0] - 92.0).abs() < EPS);
    }

    #[test]
    fn tabs_row_is_derived_from_the_replica_padding() {
        // `md` is the web's stated `h-12`; `sm` (`pt-1.5 pb-[9px]`) its 35,
        // which still falls out of the padding sum.
        let t = strip();
        assert!((t.row_thickness() - TAB_HEIGHT_MD).abs() < EPS);
        assert!((strip().small().row_thickness() - control::TAB_SM).abs() < EPS);

        // The replica's own property still wins — it is a .NET property, and it
        // must keep meaning what it means.
        let mut t = t;
        t.item_size = Size::new(0.0, 28.0);
        assert!((t.row_thickness() - 28.0).abs() < EPS);
    }

    #[test]
    fn tabs_selected_tab_and_item_at() {
        let t = strip();
        let labels = [60.0, 50.0, 80.0];
        let bounds = Rect::new(0.0, 0.0, 600.0, 200.0);
        let rects = t.tab_rects_with(&labels, bounds);
        assert_eq!(rects.len(), 3);
        assert_eq!(t.selected_index, 1);
        assert_eq!(
            t.selected_tab().map(|p| p.control().text.clone()),
            Some("Aperçu".to_string())
        );
        // Single row, top-aligned: every tab sits on the 2 DIP strip inset.
        for r in &rects {
            assert!((r.top - (bounds.top + 2.0)).abs() < EPS);
            assert!((r.bottom - r.top - TAB_HEIGHT_MD).abs() < EPS);
            // Every tab is as wide as the widest: 80 + 2 * 16.
            assert!((r.right - r.left - 112.0).abs() < EPS);
        }
        // Half-open in x, and `gap-1` apart.
        assert!(rects[0].contains(rects[0].left, rects[0].top));
        assert!(!rects[0].contains(rects[0].right, rects[0].top));
        assert!((rects[1].left - rects[0].right - TAB_GAP).abs() < EPS);
    }

    #[test]
    fn tabs_multiline_wraps_and_rotates_like_the_replica() {
        let mut t = strip();
        t.multiline = true;
        let labels = [200.0, 200.0, 200.0];
        // 232 wide each: only one fits per row.
        let bounds = Rect::new(0.0, 0.0, 300.0, 200.0);
        let extents = t.extents_with(&labels, bounds);
        let rows = tab_rows(&extents, 300.0);
        assert_eq!(rows, vec![0, 1, 2]);
        assert!((t.strip_thickness_with(&labels, bounds)
            - tab_strip_thickness(TAB_HEIGHT_MD, 3))
        .abs()
            < EPS);
        // Tab 1 is selected, so its row is the one against the page — the
        // bottom-most row of a top-aligned strip.
        let rects = t.tab_rects_with(&labels, bounds);
        assert!(rects[1].top > rects[0].top);
        assert!(rects[1].top > rects[2].top);
    }

    #[test]
    fn tabs_page_rect_is_the_replica_display_rectangle() {
        let t = strip();
        let labels = [60.0, 50.0, 80.0];
        let bounds = Rect::new(0.0, 0.0, 600.0, 400.0);
        let strip_h = t.strip_thickness_with(&labels, bounds);
        let want = tab_display_rect(bounds, TabAlignment::Top, strip_h);
        assert!((want.top - (bounds.top + strip_h)).abs() < EPS);
        assert!((strip_h - (TAB_HEIGHT_MD + 4.0)).abs() < EPS);
    }

    #[test]
    fn tabs_fit_without_scrolling() {
        let t = strip();
        assert!(t.scroll_with(&[60.0, 50.0, 80.0], Rect::new(0.0, 0.0, 600.0, 60.0)).is_none());
    }

    #[test]
    fn reveal_shows_the_start_of_a_tab_wider_than_the_viewport() {
        // Fits: the nearest edge moves in, as `revealActive` does.
        assert!((reveal(0.0, (150.0, 250.0), 200.0) - 50.0).abs() < EPS);
        assert!((reveal(120.0, (100.0, 180.0), 200.0) - 100.0).abs() < EPS);
        assert!((reveal(40.0, (60.0, 180.0), 200.0) - 40.0).abs() < EPS);
        // Wider than the viewport: its leading edge, whichever side it came from.
        assert!((reveal(0.0, (150.0, 420.0), 200.0) - 150.0).abs() < EPS);
        assert!((reveal(400.0, (150.0, 420.0), 200.0) - 150.0).abs() < EPS);
    }

    #[test]
    fn tabs_overflow_scrolls_and_reveals_the_selected_tab() {
        // Four 112 DIP tabs + 3 gaps = 460 in a 200 DIP strip.
        let mut t = strip().with("Versions précédentes");
        let labels = [80.0, 80.0, 80.0, 80.0];
        let bounds = Rect::new(10.0, 0.0, 210.0, 60.0);

        // First tab selected: no scroll, no left arrow, a right arrow.
        t.selected_index = 0;
        let s = t.scroll_with(&labels, bounds).expect("overflows");
        assert!(s.offset.abs() < EPS);
        assert!(s.left_arrow.is_none() && s.right_arrow.is_some());
        assert!((s.viewport.right - s.viewport.left - (200.0 - TAB_ARROW_WIDTH)).abs() < EPS);

        // Last tab selected, stateless caller: scrolled to the end, the last
        // tab entirely inside the viewport, only the left arrow left.
        t.selected_index = 3;
        let s = t.scroll_with(&labels, bounds).expect("overflows");
        assert!(s.left_arrow.is_some() && s.right_arrow.is_none());
        assert!((s.offset - s.max).abs() < EPS);
        let r = t.tab_rects_with(&labels, bounds)[3];
        assert!(r.left >= s.viewport.left - EPS && r.right <= s.viewport.right + EPS);

        // A caller-driven offset is honoured (clamped), not re-revealed.
        t.scroll = Some(10_000.0);
        let s2 = t.scroll_with(&labels, bounds).expect("overflows");
        assert!((s2.offset - s2.max).abs() < EPS);
        t.scroll = Some(0.0);
        let s3 = t.scroll_with(&labels, bounds).expect("overflows");
        assert!(s3.offset.abs() < EPS);
    }

    #[test]
    fn tabs_keyboard_wraps_and_skips_disabled_pages() {
        let mut t = strip().with("Partage");
        t.tab_pages[2].control_mut().enabled = false;
        t.selected_index = 1;
        assert_eq!(t.step_selection(NavKey::Next), Some(3), "skips the disabled page");
        t.selected_index = 3;
        assert_eq!(t.step_selection(NavKey::Next), Some(0), "wraps");
        assert_eq!(t.step_selection(NavKey::First), Some(0));
        assert_eq!(t.step_selection(NavKey::Last), Some(3));
        t.selected_index = 0;
        assert_eq!(t.step_selection(NavKey::Prev), Some(3), "wraps backwards");
    }

    #[test]
    fn indicator_eases_out_between_spans() {
        let a = (0.0, 100.0);
        let b = (200.0, 260.0);
        assert_eq!(ease_span(a, b, 0.0), a);
        assert_eq!(ease_span(a, b, 1.0), b);
        let mid = ease_span(a, b, 0.5);
        // Ease-out: past the halfway point at half time.
        assert!(mid.0 > 100.0 && mid.0 < 200.0);
    }

    // ── Shared interaction ──────────────────────────────────────────────────

    #[test]
    fn roving_step_follows_the_aria_patterns() {
        let usable = |i: usize| i != 2;
        assert_eq!(roving_step(5, Some(1), NavKey::Next, false, usable), Some(3));
        assert_eq!(roving_step(5, Some(4), NavKey::Next, false, usable), Some(4), "no wrap: stays");
        assert_eq!(roving_step(5, Some(4), NavKey::Next, true, usable), Some(0), "wrap");
        assert_eq!(roving_step(5, Some(0), NavKey::Prev, true, usable), Some(4));
        assert_eq!(roving_step(5, None, NavKey::Next, true, usable), Some(0), "enter at the start");
        assert_eq!(roving_step(5, None, NavKey::Prev, true, usable), Some(4), "enter at the end");
        assert_eq!(roving_step(3, Some(0), NavKey::Last, false, |_| false), None);
        assert_eq!(nav_key_of(vk::LEFT, false), Some(NavKey::Prev));
        assert_eq!(nav_key_of(vk::DOWN, true), Some(NavKey::Next));
        assert_eq!(nav_key_of(vk::DOWN, false), None, "the cross axis means nothing");
        assert_eq!(nav_key_of(vk::END, true), Some(NavKey::Last));
    }

    #[test]
    fn menus_open_below_and_flip_above_inside_the_screen() {
        let screen = Rect::new(0.0, 0.0, 1000.0, 800.0);
        let size = Size::new(200.0, 150.0);
        let r = place_menu(Rect::new(100.0, 100.0, 140.0, 136.0), size, screen);
        assert!((r.left - 100.0).abs() < EPS && (r.top - (136.0 + MENU_GAP)).abs() < EPS);
        // Near the right edge: pulled back 8 DIP inside.
        let r = place_menu(Rect::new(950.0, 100.0, 990.0, 136.0), size, screen);
        assert!((r.right - (1000.0 - MENU_VIEWPORT_EDGE)).abs() < EPS);
        // Near the bottom: above the trigger.
        let r = place_menu(Rect::new(100.0, 700.0, 140.0, 736.0), size, screen);
        assert!((r.bottom - (700.0 - MENU_GAP)).abs() < EPS);
        // The popup covers the shadow.
        let pb = menu_popup_bounds(r);
        assert!((r.left - pb.left - MENU_SHADOW_MARGIN).abs() < EPS);
    }

    // ── StatusBar ───────────────────────────────────────────────────────────

    #[test]
    fn statusbar_springs_the_leftover_width() {
        let s = StatusBar::new()
            .with(status_item("19 éléments", true))
            .with(status_item("2,4 Go", false));
        let bounds = Rect::new(0.0, 0.0, 500.0, height::BUTTON_SM);
        let l = s.item_rects_with(&[100.0, 80.0], bounds);
        // The springing cell absorbs everything the other one leaves.
        assert!((l.rects[0].right - l.rects[0].left - 420.0).abs() < EPS);
        assert!((l.rects[1].right - l.rects[1].left - 80.0).abs() < EPS);
        assert!((l.rects[1].right - bounds.right).abs() < EPS);
    }

    #[test]
    fn statusbar_right_aligned_cells_pack_from_the_trailing_edge() {
        let mut s = StatusBar::new()
            .with(status_item("19 éléments", true))
            .with(icon_label_item("Git", "3 / 0"));
        match &mut s.items[1] {
            StripItem::Button(b) => b.item.alignment = ToolStripItemAlignment::Right,
            _ => unreachable!(),
        }
        let bounds = Rect::new(0.0, 0.0, 400.0, 32.0);
        let l = s.item_rects_with(&[100.0, 90.0], bounds);
        assert!((l.rects[1].right - bounds.right).abs() < EPS);
        assert!((l.rects[1].left - (bounds.right - 90.0)).abs() < EPS);
    }

    #[test]
    fn statusbar_item_at_is_half_open() {
        let s = StatusBar::new()
            .with(status_item("a", false))
            .with(status_item("b", false));
        let bounds = Rect::new(0.0, 0.0, 400.0, 32.0);
        let l = s.item_rects_with(&[100.0, 100.0], bounds);
        assert!(l.rects[0].contains(0.0, 0.0));
        assert!(!l.rects[0].contains(100.0, 0.0));
        assert!(l.rects[1].contains(100.0, 0.0));
    }

    #[test]
    fn statusbar_only_springs_shrink_when_too_narrow() {
        let s = StatusBar::new()
            .with(status_item("9 membres", false))
            .with(status_item("1 sélectionné", true))
            .with(status_item("Synchronisé", false));
        let bounds = Rect::new(0.0, 0.0, 200.0, 32.0);
        let l = s.item_rects_with(&[80.0, 100.0, 90.0], bounds);
        // The fixed cells keep their full width; the spring gives the 70 over.
        assert!((l.rects[0].right - l.rects[0].left - 80.0).abs() < EPS);
        assert!((l.rects[1].right - l.rects[1].left - 30.0).abs() < EPS);
        assert!((l.rects[2].right - l.rects[2].left - 90.0).abs() < EPS);
        assert!((l.rects[2].right - bounds.right).abs() < EPS);
        // Pure helper: a spring never goes negative, the deficit it cannot pay
        // is left (and clipped by the paint).
        let w = shrink_springs(&[80.0, 10.0, 90.0], &[(true, false), (true, true), (true, false)], 100.0);
        assert_eq!(w, vec![80.0, 0.0, 90.0]);
    }

    // ── Toolbar: band, keyboard, overflow menu ──────────────────────────────

    #[test]
    fn toolbar_is_transparent_unless_asked() {
        assert!(!Toolbar::new().band, "no card band inside a card");
        assert!(Toolbar::new().with_band(true).band);
    }

    #[test]
    fn toolbar_focus_order_skips_separators_labels_and_disabled() {
        let mut t = Toolbar::new()
            .with(icon_item("Cut"))
            .with(separator_item())
            .with(label_item("Texte"))
            .with(icon_item("Copy"))
            .with(icon_item("Paste"));
        if let StripItem::Button(b) = &mut t.items[3] {
            b.item.enabled = false;
        }
        let a = t.arrange_widths(&widths(&t, 40.0), Rect::new(0.0, 0.0, 600.0, 36.0));
        assert_eq!(t.focus_order(&a), vec![ToolbarTarget::Item(0), ToolbarTarget::Item(4)]);
    }

    #[test]
    fn toolbar_overflow_menu_maps_rows_back_to_items() {
        let t = Toolbar::new()
            .with(icon_item("Cut"))
            .with(with_tooltip(icon_item("Copy"), "Copier"))
            .with(separator_item())
            .with(icon_label_item("Share2", "Partager"))
            .with(separator_item());
        // Everything but the first item overflows.
        let a = ToolbarArrangement {
            visible: vec![(0, Rect::new(0.0, 0.0, 40.0, 36.0))],
            overflow: vec![1, 2, 3, 4],
            overflow_button: Some(Rect::new(40.0, 0.0, 80.0, 36.0)),
        };
        let m = t.overflow_menu(&a);
        // Copier, separator, Partager — the trailing rule is dropped.
        assert_eq!(m.items().len(), 3);
        assert_eq!(m.items()[0].item().text, "Copier", "an icon command reads as its tooltip");
        assert!(matches!(m.items()[1], StripItem::Separator(_)));
        assert_eq!(t.overflow_item_for_row(&a, 0), Some(1));
        assert_eq!(t.overflow_item_for_row(&a, 1), None, "a rule is no command");
        assert_eq!(t.overflow_item_for_row(&a, 2), Some(3));
        assert_eq!(t.focus_order(&a).last(), Some(&ToolbarTarget::Overflow));
    }

    // ── Sidebar: keyboard and scroll ────────────────────────────────────────

    #[test]
    fn sidebar_keyboard_skips_headers_and_does_not_wrap() {
        let s = pane();
        assert_eq!(s.step_focus(Some(0), NavKey::Next), Some(2), "the header is inert");
        assert_eq!(s.step_focus(Some(2), NavKey::Next), Some(2), "no wrap");
        assert_eq!(s.step_focus(None, NavKey::First), Some(0));
        assert_eq!(s.step_focus(Some(2), NavKey::Prev), Some(0));
    }

    #[test]
    fn sidebar_scrolls_its_rows() {
        let mut s = pane();
        let bounds = Rect::new(0.0, 0.0, 260.0, 60.0);
        assert!(s.max_scroll(bounds) > 0.0);
        let before = s.row_rects(bounds)[2];
        let reveal = s.reveal_row(bounds, 2);
        assert!(reveal > 0.0 && reveal <= s.max_scroll(bounds) + EPS);
        s.scroll_y = reveal;
        let after = s.row_rects(bounds)[2];
        assert!((before.top - after.top - reveal).abs() < EPS);
        assert!(after.bottom <= bounds.bottom + EPS);
        // A row scrolled out of the pane is not under the pointer.
        assert_eq!(s.item_at(bounds, 20.0, -10.0), None);
    }

    // ── Breadcrumb: links, folded head ──────────────────────────────────────

    #[test]
    fn breadcrumb_folds_into_a_menu_and_the_tail_is_not_a_link() {
        let b = trail();
        let w = [80.0, 100.0, 90.0, 110.0];
        let l = b.layout_with(&w, Rect::new(0.0, 0.0, 200.0, 32.0));
        let hidden = b.hidden_segments(&l);
        assert_eq!(hidden, 0..l.start_index);
        assert_eq!(b.hidden_menu(&l).items().len(), l.start_index);
        assert!(!b.is_link(3), "the current page is not a destination");
        assert!(b.is_link(0));
        let order = b.focus_order(&l);
        assert_eq!(order.first(), Some(&None), "the … button comes first");
        assert!(!order.contains(&Some(3)));
    }

    // ── The `Widget` contract ───────────────────────────────────────────────

    #[test]
    fn every_primitive_names_itself_and_its_model() {
        assert_eq!(Toolbar::new().type_name(), "Toolbar");
        assert_eq!(Sidebar::new().type_name(), "Sidebar");
        assert_eq!(Breadcrumb::new().type_name(), "Breadcrumb");
        assert_eq!(Tabs::new().type_name(), "Tabs");
        assert_eq!(StatusBar::new().type_name(), "StatusBar");

        assert_eq!(Toolbar::new().model().type_name(), "ToolStrip");
        assert_eq!(Sidebar::new().model().type_name(), "ToolStrip");
        assert_eq!(Breadcrumb::new().model().type_name(), "ToolStrip");
        assert_eq!(Tabs::new().model().type_name(), "TabControl");
        assert_eq!(StatusBar::new().model().type_name(), "StatusStrip");
    }

    #[test]
    fn the_replica_defaults_each_primitive_changes_are_the_ones_it_declares() {
        let t = Toolbar::new();
        assert_eq!(t.grip_style, ToolStripGripStyle::Hidden);
        assert!(t.can_overflow, "a tool bar overflows into a More menu");

        let s = Sidebar::new();
        assert_eq!(s.layout_style, ToolStripLayoutStyle::VerticalStackWithOverflow);
        assert!(!s.can_overflow, "a pane scrolls, it does not fold rows away");

        // `StatusStrip` already brings the springing Table layout and the
        // bottom dock; the Kubuno bar changes none of it.
        let b = StatusBar::new();
        assert_eq!(b.layout_style, ToolStripLayoutStyle::Table);
        assert!(b.sizing_grip);
    }
}
