//! The dock — a port of the web's shared panel system
//! (`core/frontend/src/core/shell/workspace/Dock.tsx`, `DockArea`), the
//! draggable / dockable panels every advanced editor wears (PaintSharp's
//! sub-editors, the App builder…).
//!
//! # What it is
//!
//! Panels are TABS that can be re-docked left or right, merged into a tab
//! group, split above / below another group, torn off as floating windows,
//! rolled up or maximised (floats), resized (columns and stacked groups),
//! closed and reopened — with a single ghost rectangle showing the exact
//! landing zone during a drag, and the Visual Studio « guide diamond »: a
//! five-way compass over the pane under the pointer plus two window-edge
//! arrows.
//!
//! The host declares a panel registry ([`DockPanel`]: id → label) and a
//! default arrangement ([`DockArrangement`]); [`DockArea`] owns the layout
//! state ([`DockLayout`], serde-serialisable in the web's own JSON shape so a
//! layout saved by one is read by the other), its persistence, the drag logic
//! and the « viewport with docks » row.
//!
//! # Immediate mode
//!
//! [`DockArea::frame`] handles the frame's input, paints the chrome and calls
//! the caller's `content` closure once per visible surface — the viewport
//! (the dock's children on the web) and the active panel of every group — in
//! z-order, clipped to the surface, with a [`Frame`] whose pointer is masked
//! when something else (a float, a drag in progress, the tab menu) sits above
//! it. Floating panels are painted after the docked ones, and the ghost and
//! the guides after everything.
//!
//! # Floating windows
//!
//! Floats are **in-window** surfaces, like the web's `position: fixed`
//! floats: they live inside the dock's bounds, snap to its edges and to one
//! another, and are persisted relative to the dock. They are not top-level OS
//! windows: a panel's content is painted by the caller's closure every frame
//! (immediate mode), and the host's top-level surfaces ([`host::popup`]) only
//! take `'static` paint closures — a torn-off OS window would need the panel
//! to be a retained, self-painting object, which the web's panels are not.
//!
//! # Differences with the web, on purpose
//!
//! * The web publishes the closed panels to the shell's right rail
//!   (`dockReopenStore`); a desktop app has no such rail, so the dock exposes
//!   [`DockArea::closed_panels`] and paints its own reopen button
//!   ([`DockArea::paint_reopen_button`]) wherever the host puts it.
//! * Keyboard: Ctrl+Tab / Ctrl+Shift+Tab cycle through the panels (surfacing
//!   the next one, as Visual Studio does), Escape cancels a drag or a resize,
//!   the Menu key (or Shift+F10) opens the focused tab's menu.
//! * A panel may carry an icon and may be declared non-closable — what the
//!   designable `<DockPanel>` offers.

use std::sync::atomic::{AtomicU32, Ordering};

use drive_app_controls::{Canvas, Rect, Theme};
use kubuno_controls::host::{self, access, vk, Cursor, Frame, InputEvent, Modifiers};
use serde::{Deserialize, Serialize};
use windows::core::HSTRING;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT, DWRITE_WORD_WRAPPING_NO_WRAP,
};

use crate::lists::{self, Menu, MenuEntry, MenuKey, MenuOutcome};
use kubuno_controls::toolstrip::StripItem;
use crate::metrics::ShadowLayer;
use crate::{Widget, WidgetState};

// ═════════════════════════════════════════════════════════════════════════════
// Metrics — the web's constants and Tailwind classes, in DIP
// ═════════════════════════════════════════════════════════════════════════════

pub mod metrics {
    /// `MIN_W` / `MAX_W` / `DEF_W`: a column's width bounds and default; also
    /// a float's width.
    pub const MIN_W: f32 = 190.0;
    pub const MAX_W: f32 = 480.0;
    pub const DEF_W: f32 = 256.0;
    /// `MIN_H`: the smallest height of a stacked group while resizing.
    pub const MIN_H: f32 = 60.0;
    /// `theme.radius ?? 12` / `theme.gap ?? 12`.
    pub const RADIUS: f32 = 12.0;
    pub const GAP: f32 = 12.0;
    /// A tab: `height: 34`, `padding: 0 12px`, `fontSize: 13`, label
    /// `max-w-[150px]`, `gap-1.5` before the close button.
    pub const TAB_H: f32 = 34.0;
    pub const TAB_PAD_X: f32 = 12.0;
    pub const TAB_FONT: f32 = 13.0;
    pub const TAB_LABEL_MAX: f32 = 150.0;
    pub const TAB_INNER_GAP: f32 = 6.0;
    /// The strip's `gap: 4` between tabs (and between wrapped rows).
    pub const TAB_GAP: f32 = 4.0;
    /// A docked strip's `padding: 6px 6px 0`.
    pub const STRIP_PAD: f32 = 6.0;
    /// The close button: `p-0.5` around `<X size={12}/>`, `rounded-full`.
    pub const CLOSE_BTN: f32 = 16.0;
    pub const CLOSE_ICON: f32 = 12.0;
    /// The active tab's underline: `left: 10, right: 10, height: 3`.
    pub const UNDERLINE_INSET: f32 = 10.0;
    pub const UNDERLINE_H: f32 = 3.0;
    /// A float's caption buttons: `w-7 h-7`, chevrons 14, copy/square 12,
    /// `gap-0.5`, `pr-1`, and the strip's own `padding-right: 4`.
    pub const CAPTION_BTN: f32 = 28.0;
    pub const CAPTION_GAP: f32 = 2.0;
    pub const CAPTION_PAD_R: f32 = 4.0;
    /// `maxHeight: 72vh` of a float.
    pub const FLOAT_MAX_H: f32 = 0.72;
    /// A float's default height when nothing better is known.
    pub const FLOAT_DEF_H: f32 = 300.0;
    /// The smallest float (so its caption and a line of content stay usable).
    pub const FLOAT_MIN_H: f32 = 120.0;
    /// The resize grip: a 5 DIP hairline and a `h-9 w-3.5` pill (rotated for
    /// rows), `GripVertical size={13}`.
    pub const GRIP_LINE: f32 = 5.0;
    pub const GRIP_LONG: f32 = 36.0;
    pub const GRIP_SHORT: f32 = 14.0;
    pub const GRIP_ICON: f32 = 13.0;
    /// A drag starts after 5 DIP of travel.
    pub const DRAG_SLOP: f32 = 5.0;
    /// `SNAP`: magnetic distance of a float to an edge.
    pub const SNAP: f32 = 9.0;
    /// `GUIDE_HIT`: how close to a guide's centre the pointer must be.
    pub const GUIDE_HIT: f32 = 20.0;
    /// A guide: `width: 30, height: 30`, `rounded-md`; the compass arms are
    /// `D = 38` from the pane centre, the edge arrows 24 from the window edge.
    pub const GUIDE: f32 = 30.0;
    pub const GUIDE_RADIUS: f32 = 6.0;
    pub const GUIDE_ARM: f32 = 38.0;
    pub const GUIDE_EDGE: f32 = 24.0;
    pub const GUIDE_ACTIVE_SCALE: f32 = 1.12;
    /// The ghost: `borderRadius: 8`, `2px` border.
    pub const GHOST_RADIUS: f32 = 8.0;
    pub const GHOST_BORDER: f32 = 2.0;
    /// Where a float lands relative to the pointer: `x - fw/2`, `y - 14`, and
    /// at least 8 DIP inside the dock.
    pub const FLOAT_GRAB_Y: f32 = 14.0;
    pub const FLOAT_MARGIN: f32 = 8.0;
    /// The reopen button (the right rail's `h-10 w-10`, icon 20, badge 16).
    pub const REOPEN_BTN: f32 = 40.0;
    pub const REOPEN_ICON: f32 = 20.0;
    pub const REOPEN_BADGE: f32 = 16.0;
}

use metrics as m;

// ═════════════════════════════════════════════════════════════════════════════
// The model — `DockLayout`, `DockGroup`, `DropTarget`, and the pure transforms
// ═════════════════════════════════════════════════════════════════════════════

/// A panel id (editor-defined, a plain string like on the web).
pub type PanelId = String;

/// `DockSideKey`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DockSide {
    Left,
    Right,
    Float,
}

/// Above or below a group, for a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Where {
    Top,
    Bottom,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// `DockGroup`: a tab group — docked in a column or floating.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DockGroup {
    pub id: String,
    pub panels: Vec<PanelId>,
    pub active: PanelId,
    /// A float's position, in DIP from the dock's top-left corner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    /// Docked: the group's height WEIGHT in its column (`flex: h 1 0`).
    /// Floating: the float's height in DIP (the web sizes a float by its
    /// content, which an immediate-mode panel does not have).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f32>,
    /// A float rolled up to its header bar.
    #[serde(default, skip_serializing_if = "is_false")]
    pub rolled: bool,
    /// A float maximised to the dock.
    #[serde(default, skip_serializing_if = "is_false")]
    pub max: bool,
}

impl DockGroup {
    fn of(panels: Vec<PanelId>) -> Self {
        let active = panels.first().cloned().unwrap_or_default();
        Self { id: new_gid(), panels, active, ..Self::default() }
    }
}

/// `DockLayout`: where every panel is. Serialised exactly like the web's
/// (`left`, `right`, `float`, `leftW`, `rightW`, `closed`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DockLayout {
    #[serde(default)]
    pub left: Vec<DockGroup>,
    #[serde(default)]
    pub right: Vec<DockGroup>,
    #[serde(default)]
    pub float: Vec<DockGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub left_w: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub right_w: Option<f32>,
    /// Panels the user closed (re-openable).
    #[serde(default)]
    pub closed: Vec<PanelId>,
}

impl DockLayout {
    pub fn side(&self, side: DockSide) -> &Vec<DockGroup> {
        match side {
            DockSide::Left => &self.left,
            DockSide::Right => &self.right,
            DockSide::Float => &self.float,
        }
    }

    pub fn side_mut(&mut self, side: DockSide) -> &mut Vec<DockGroup> {
        match side {
            DockSide::Left => &mut self.left,
            DockSide::Right => &mut self.right,
            DockSide::Float => &mut self.float,
        }
    }

    /// The layout as JSON — what the web keeps in `localStorage`.
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    /// A layout read back from JSON (not reconciled: see [`reconcile`]).
    pub fn from_json(json: &str) -> Option<Self> {
        serde_json::from_str(json).ok()
    }

    /// Every placed panel, in order: left column, right column, floats.
    pub fn placed(&self) -> Vec<&PanelId> {
        [&self.left, &self.right, &self.float].into_iter().flatten().flat_map(|g| g.panels.iter()).collect()
    }

    /// Where `id` is: its side and group index.
    pub fn find(&self, id: &str) -> Option<(DockSide, usize)> {
        for side in [DockSide::Left, DockSide::Right, DockSide::Float] {
            if let Some(i) = self.side(side).iter().position(|g| g.panels.iter().any(|p| p == id)) {
                return Some((side, i));
            }
        }
        None
    }

    /// The active panel of every group, in order (what is on screen).
    pub fn visible(&self) -> Vec<&PanelId> {
        [&self.left, &self.right, &self.float].into_iter().flatten().map(|g| &g.active).collect()
    }
}

/// `DropTarget`: where a dragged panel lands.
#[derive(Debug, Clone, PartialEq)]
pub enum DropTarget {
    /// Merged into group `gid` as a tab.
    Tabs { side: DockSide, gid: String },
    /// A new group above / below group `gid`.
    Split { side: DockSide, gid: String, at: Where },
    /// A new group at the end of a column.
    NewCol { side: DockSide },
    /// A new float at `(x, y)` (dock-relative).
    Float { x: f32, y: f32 },
}

/// `defaultArrangement`: the groups of each side, each a list of panel ids.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DockArrangement {
    pub left: Vec<Vec<PanelId>>,
    pub right: Vec<Vec<PanelId>>,
    pub float: Vec<Vec<PanelId>>,
}

impl DockArrangement {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a group of `panels` at the end of the left column.
    pub fn left(mut self, panels: &[&str]) -> Self {
        self.left.push(panels.iter().map(|p| p.to_string()).collect());
        self
    }

    /// Adds a group of `panels` at the end of the right column.
    pub fn right(mut self, panels: &[&str]) -> Self {
        self.right.push(panels.iter().map(|p| p.to_string()).collect());
        self
    }

    /// Adds a floating group of `panels`.
    pub fn floating(mut self, panels: &[&str]) -> Self {
        self.float.push(panels.iter().map(|p| p.to_string()).collect());
        self
    }

    /// Adds `panel` to the group `side`'s LAST group, or a new group when the
    /// side has none (how a declarative `Group` groups panels).
    pub fn push(&mut self, side: DockSide, panel: &str, new_group: bool) {
        let v = match side {
            DockSide::Left => &mut self.left,
            DockSide::Right => &mut self.right,
            DockSide::Float => &mut self.float,
        };
        match v.last_mut() {
            Some(g) if !new_group => g.push(panel.to_string()),
            _ => v.push(vec![panel.to_string()]),
        }
    }
}

static GID: AtomicU32 = AtomicU32::new(1);

/// `newGid`: a fresh group id (`"g<n>"`).
pub fn new_gid() -> String {
    format!("g{}", GID.fetch_add(1, Ordering::Relaxed))
}

/// Moves the id counter past `gid` (`"g<n>"`), so an id read back from a
/// saved layout is never handed out again. The web restarts its counter at
/// each page load and relies on `reconcile` to catch the collision; here a
/// layout restored at start-up would otherwise collide with the next float.
fn reserve_gid(gid: &str) {
    if let Some(n) = gid.strip_prefix('g').and_then(|n| n.parse::<u32>().ok()) {
        GID.fetch_max(n.saturating_add(1), Ordering::Relaxed);
    }
}

const SIDES: [DockSide; 3] = [DockSide::Left, DockSide::Right, DockSide::Float];

/// `activatePanel`: `id` becomes the active tab of its group.
pub fn activate_panel(layout: &DockLayout, id: &str) -> DockLayout {
    let mut l = layout.clone();
    for side in SIDES {
        for g in l.side_mut(side) {
            if g.panels.iter().any(|p| p == id) {
                g.active = id.to_string();
            }
        }
    }
    l
}

/// `removePanel`: takes `id` out of its group, dropping a group left empty.
pub fn remove_panel(layout: &DockLayout, id: &str) -> DockLayout {
    let mut l = layout.clone();
    for side in SIDES {
        let v = l.side_mut(side);
        for g in v.iter_mut() {
            if g.panels.iter().any(|p| p == id) {
                g.panels.retain(|p| p != id);
                if g.active == id {
                    g.active = g.panels.first().cloned().unwrap_or_default();
                }
            }
        }
        v.retain(|g| !g.panels.is_empty());
    }
    l
}

/// `applyDrop`: moves `id` to `tgt`.
pub fn apply_drop(layout: &DockLayout, id: &str, tgt: &DropTarget) -> DockLayout {
    let mut l = remove_panel(layout, id);
    let mk = || DockGroup::of(vec![id.to_string()]);
    match tgt {
        DropTarget::Float { x, y } => {
            l.float.push(DockGroup { x: Some(*x), y: Some(*y), ..mk() });
        }
        DropTarget::NewCol { side } => {
            l.side_mut(*side).push(mk());
        }
        DropTarget::Tabs { side, gid } | DropTarget::Split { side, gid, .. } => {
            let arr = l.side_mut(*side);
            let Some(gi) = arr.iter().position(|g| &g.id == gid) else {
                // The target group vanished (it held only the dragged panel):
                // land in a new group on that side (floats go right).
                let side = if *side == DockSide::Float { DockSide::Right } else { *side };
                l.side_mut(side).push(mk());
                return l;
            };
            match tgt {
                DropTarget::Tabs { .. } => {
                    arr[gi].panels.push(id.to_string());
                    arr[gi].active = id.to_string();
                }
                DropTarget::Split { at, .. } => {
                    let at = if *at == Where::Top { gi } else { gi + 1 };
                    arr.insert(at, mk());
                }
                _ => {}
            }
        }
    }
    l
}

/// `closePanel`: parks `id` in `closed` (re-openable).
pub fn close_panel(layout: &DockLayout, id: &str) -> DockLayout {
    let mut l = remove_panel(layout, id);
    l.closed.retain(|p| p != id);
    l.closed.push(id.to_string());
    l
}

/// `isDocked`: is the panel placed anywhere?
pub fn is_docked(layout: &DockLayout, id: &str) -> bool {
    layout.find(id).is_some()
}

/// `openPanel`: a panel is ONE live instance — opening one that is already on
/// screen SURFACES it (active tab of its group, a rolled float unrolled);
/// otherwise it is re-docked in a new group of the right column.
pub fn open_panel(layout: &DockLayout, id: &str) -> DockLayout {
    let mut l = layout.clone();
    l.closed.retain(|p| p != id);
    if is_docked(&l, id) {
        for g in l.float.iter_mut() {
            if g.panels.iter().any(|p| p == id) {
                g.rolled = false;
            }
        }
        return activate_panel(&l, id);
    }
    l.right.push(DockGroup::of(vec![id.to_string()]));
    l
}

/// `buildDefault`.
pub fn build_default(arr: &DockArrangement) -> DockLayout {
    let mk = |gs: &Vec<Vec<PanelId>>| gs.iter().filter(|p| !p.is_empty()).map(|p| DockGroup::of(p.clone())).collect();
    DockLayout {
        left: mk(&arr.left),
        right: mk(&arr.right),
        float: mk(&arr.float),
        left_w: Some(m::DEF_W),
        right_w: Some(m::DEF_W),
        closed: Vec::new(),
    }
}

/// `reconcile`: drops panels no longer known, appends known panels missing
/// from the layout (unless the user closed them) at the top of the right
/// column, and enforces « one panel, one place » and unique group ids — a
/// stored layout may hold duplicates.
pub fn reconcile(layout: &DockLayout, known: &[&str]) -> DockLayout {
    let is_known = |p: &str| known.contains(&p);
    for g in SIDES.iter().flat_map(|s| layout.side(*s).iter()) {
        reserve_gid(&g.id);
    }
    let mut out = DockLayout {
        left_w: Some(layout.left_w.unwrap_or(m::DEF_W)),
        right_w: Some(layout.right_w.unwrap_or(m::DEF_W)),
        ..DockLayout::default()
    };
    let mut present: Vec<PanelId> = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let fresh = |used: &Vec<String>| {
        let mut id = new_gid();
        while used.contains(&id) {
            id = new_gid();
        }
        id
    };
    for side in SIDES {
        let mut groups = Vec::new();
        for g in layout.side(side) {
            let panels: Vec<PanelId> = g
                .panels
                .iter()
                .filter(|p| {
                    if !is_known(p) || present.contains(p) {
                        return false;
                    }
                    present.push((*p).clone());
                    true
                })
                .cloned()
                .collect();
            let gid = if g.id.is_empty() || used.contains(&g.id) { fresh(&used) } else { g.id.clone() };
            used.push(gid.clone());
            if panels.is_empty() {
                continue;
            }
            let active = if panels.contains(&g.active) { g.active.clone() } else { panels[0].clone() };
            groups.push(DockGroup { id: gid, panels, active, ..g.clone() });
        }
        *out.side_mut(side) = groups;
    }
    out.closed = layout
        .closed
        .iter()
        .filter(|p| {
            if !is_known(p) || present.contains(p) {
                return false;
            }
            present.push((*p).clone());
            true
        })
        .cloned()
        .collect();
    for p in known {
        if !present.iter().any(|q| q == p) {
            let gid = fresh(&used);
            used.push(gid.clone());
            out.right.insert(0, DockGroup { id: gid, panels: vec![p.to_string()], active: p.to_string(), ..DockGroup::default() });
        }
    }
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// The theme — `DockTheme` (Direction A: « Material-refined chrome »)
// ═════════════════════════════════════════════════════════════════════════════

/// `0xRRGGBB` as an opaque colour.
pub const fn hex(rgb: u32) -> D2D1_COLOR_F {
    D2D1_COLOR_F {
        r: ((rgb >> 16) & 0xff) as f32 / 255.0,
        g: ((rgb >> 8) & 0xff) as f32 / 255.0,
        b: (rgb & 0xff) as f32 / 255.0,
        a: 1.0,
    }
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

/// `color-mix(in srgb, a p, b)`.
pub fn mix(a: D2D1_COLOR_F, b: D2D1_COLOR_F, p: f32) -> D2D1_COLOR_F {
    let q = 1.0 - p;
    D2D1_COLOR_F { r: a.r * p + b.r * q, g: a.g * p + b.g * q, b: a.b * p + b.b * q, a: a.a * p + b.a * q }
}

/// `DockTheme`: the base tokens, plus the optional Direction-A ones (derived
/// from the base when absent, exactly as the web derives them).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DockTheme {
    pub panel: D2D1_COLOR_F,
    pub header: D2D1_COLOR_F,
    pub border: D2D1_COLOR_F,
    pub text: D2D1_COLOR_F,
    pub text_dim: D2D1_COLOR_F,
    pub accent: D2D1_COLOR_F,
    /// Neutral behind the panel cards and gutters (`color-mix(border 45%, panel)`).
    pub ground: Option<D2D1_COLOR_F>,
    /// Panel-card corner radius.
    pub radius: Option<f32>,
    /// Gutter between blocks (locked to 12 by the design).
    pub gap: Option<f32>,
    /// Active-tab pill (`color-mix(accent 12%, panel)`).
    pub tab_active_bg: Option<D2D1_COLOR_F>,
}

impl DockTheme {
    /// `DEFAULT_THEME`: follows the app's theme (light or dark) through the
    /// core semantic tokens — `surface-0`, `surface-2`, `border`,
    /// `text-primary`, `text-secondary`, `primary`.
    pub fn from_theme(t: &Theme) -> Self {
        Self {
            panel: t.layer_background,
            header: t.surface_2,
            border: t.card_stroke,
            text: t.text_primary,
            text_dim: t.text_secondary,
            accent: t.accent,
            ground: None,
            radius: None,
            gap: None,
            tab_active_bg: None,
        }
    }

    /// The PaintSharp / `WORKSPACE_DARK` palette (Photoshop-like).
    pub fn workspace_dark() -> Self {
        Self {
            panel: hex(0x323232),
            header: hex(0x2b2b2b),
            border: hex(0x212121),
            text: hex(0xd6d6d6),
            text_dim: hex(0x8e8e8e),
            accent: hex(0x5a9bdc),
            ground: None,
            radius: None,
            gap: None,
            tab_active_bg: None,
        }
    }

    /// The `WORKSPACE_LIGHT` palette the App builder passes.
    pub fn workspace_light() -> Self {
        Self {
            panel: hex(0xf8f9fa),
            header: hex(0xf1f3f4),
            border: hex(0xdadce0),
            text: hex(0x202124),
            text_dim: hex(0x5f6368),
            accent: hex(0x1a73e8),
            ground: None,
            radius: None,
            gap: None,
            tab_active_bg: None,
        }
    }

    fn resolve(&self) -> Colors {
        let accent = self.accent;
        let tab_active_bg = self.tab_active_bg.unwrap_or_else(|| mix(accent, self.panel, 0.12));
        Colors {
            panel: self.panel,
            border: self.border,
            text_dim: self.text_dim,
            accent,
            ground: self.ground.unwrap_or_else(|| mix(self.border, self.panel, 0.45)),
            tab_active_bg,
            float_strip: mix(accent, self.panel, 0.10),
            grip_line: mix(accent, self.border, 0.55),
            grip_border: mix(accent, self.border, 0.40),
            hover: rgba(0, 0, 0, 0.10),
            radius: self.radius.unwrap_or(m::RADIUS),
            gap: self.gap.unwrap_or(m::GAP),
        }
    }
}

/// The resolved colours of one frame.
#[derive(Debug, Clone, Copy)]
struct Colors {
    panel: D2D1_COLOR_F,
    border: D2D1_COLOR_F,
    text_dim: D2D1_COLOR_F,
    accent: D2D1_COLOR_F,
    ground: D2D1_COLOR_F,
    tab_active_bg: D2D1_COLOR_F,
    float_strip: D2D1_COLOR_F,
    grip_line: D2D1_COLOR_F,
    grip_border: D2D1_COLOR_F,
    hover: D2D1_COLOR_F,
    radius: f32,
    gap: f32,
}

/// `cardShadow`: `0 1px 3px rgba(16,24,40,.08), 0 1px 2px rgba(16,24,40,.06)`.
const CARD_SHADOW: [ShadowLayer; 2] = [
    ShadowLayer { dy: 1.0, blur: 3.0, spread: 0.0, opacity: 0.08 },
    ShadowLayer { dy: 1.0, blur: 2.0, spread: 0.0, opacity: 0.06 },
];
/// A float's `0 24px 50px -12px rgba(16,24,40,.34), 0 6px 16px -6px rgba(16,24,40,.20)`.
const FLOAT_SHADOW: [ShadowLayer; 2] = [
    ShadowLayer { dy: 24.0, blur: 50.0, spread: -12.0, opacity: 0.34 },
    ShadowLayer { dy: 6.0, blur: 16.0, spread: -6.0, opacity: 0.20 },
];
/// `rgba(16,24,40)`, the shadows' ink.
const SHADOW_INK: (f32, f32, f32) = (16.0 / 255.0, 24.0 / 255.0, 40.0 / 255.0);
/// A guide's `shadow-md`.
const GUIDE_SHADOW: [ShadowLayer; 2] = [
    ShadowLayer { dy: 4.0, blur: 6.0, spread: -1.0, opacity: 0.10 },
    ShadowLayer { dy: 2.0, blur: 4.0, spread: -2.0, opacity: 0.10 },
];

/// The guides and the ghost are NOT themed on the web (fixed colours).
mod fixed {
    use super::{hex, rgba, D2D1_COLOR_F};
    pub const GHOST_FILL: D2D1_COLOR_F = rgba(90, 160, 255, 0.22);
    pub const GHOST_BORDER: D2D1_COLOR_F = rgba(90, 160, 255, 0.95);
    pub const GUIDE_BG: D2D1_COLOR_F = rgba(255, 255, 255, 0.96);
    pub const GUIDE_ACTIVE: D2D1_COLOR_F = hex(0x1a73e8);
    pub const GUIDE_INK: D2D1_COLOR_F = hex(0x5f6368);
    pub const GUIDE_BORDER: D2D1_COLOR_F = hex(0xbdc1c6);
    pub const WHITE: D2D1_COLOR_F = hex(0xffffff);
    /// `viewportBg = '#141414'`, the web's default.
    pub const VIEWPORT: D2D1_COLOR_F = hex(0x141414);
}

// ═════════════════════════════════════════════════════════════════════════════
// Text formats
// ═════════════════════════════════════════════════════════════════════════════

type FormatCache = (Option<IDWriteFactory>, String, Vec<((u32, u32), IDWriteTextFormat)>);

thread_local! {
    static FORMATS: std::cell::RefCell<FormatCache> = const { std::cell::RefCell::new((None, String::new(), Vec::new())) };
}

fn body_family(c: &dyn Canvas) -> String {
    let f = &c.formats().body;
    // SAFETY: plain COM getters on a live text format.
    unsafe {
        let n = f.GetFontFamilyNameLength() as usize;
        let mut buf = vec![0u16; n + 1];
        if f.GetFontFamilyName(&mut buf).is_err() {
            return String::from("Segoe UI");
        }
        String::from_utf16_lossy(&buf[..n])
    }
}

/// A one-line text format of the UI family at `size` DIP and `weight`,
/// cached; falls back to a shared format when DirectWrite refuses.
pub(crate) fn ui_format(c: &dyn Canvas, size: f32, weight: u32) -> IDWriteTextFormat {
    let key = ((size * 100.0).round() as u32, weight);
    let family = body_family(c);
    let made = FORMATS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.1 != family {
            cache.1 = family.clone();
            cache.2.clear();
        }
        if let Some((_, f)) = cache.2.iter().find(|(k, _)| *k == key) {
            return Some(f.clone());
        }
        if cache.0.is_none() {
            // SAFETY: creating the shared DirectWrite factory has no preconditions.
            cache.0 = unsafe { DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED) }.ok();
        }
        let factory = cache.0.clone()?;
        // SAFETY: plain COM calls on a live factory.
        let f = unsafe {
            factory
                .CreateTextFormat(
                    &HSTRING::from(family.as_str()),
                    None,
                    DWRITE_FONT_WEIGHT(weight as i32),
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    size,
                    &HSTRING::from("fr-FR"),
                )
                .and_then(|f| f.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).map(|_| f))
        }
        .ok()?;
        cache.2.push((key, f.clone()));
        Some(f)
    });
    made.unwrap_or_else(|| if size >= 14.0 { c.formats().body.clone() } else { c.formats().caption.clone() })
}

// ═════════════════════════════════════════════════════════════════════════════
// The panel registry and the strings
// ═════════════════════════════════════════════════════════════════════════════

/// `DockPanel`: one entry of the registry — the web's `{ label, render }`,
/// the content being painted by the caller's closure.
#[derive(Debug, Clone, PartialEq)]
pub struct DockPanel {
    pub id: PanelId,
    pub label: String,
    /// A Lucide geometry name shown before the label (desktop addition).
    pub icon: Option<&'static str>,
    /// Whether the tab shows a close button and the menu a « close » row.
    pub closable: bool,
}

impl DockPanel {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self { id: id.into(), label: label.into(), icon: None, closable: true }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }
}

/// The dock's user-facing strings (the web's French literals by default).
#[derive(Debug, Clone, PartialEq)]
pub struct DockStrings {
    pub float: String,
    pub dock_left: String,
    pub dock_right: String,
    pub close_panel: String,
    pub reset: String,
    pub close: String,
    pub roll: String,
    pub unroll: String,
    pub maximize: String,
    pub restore: String,
    pub reopen: String,
}

impl Default for DockStrings {
    fn default() -> Self {
        Self {
            float: "Détacher (flottant)".into(),
            dock_left: "Ancrer à gauche".into(),
            dock_right: "Ancrer à droite".into(),
            close_panel: "Fermer le panneau".into(),
            reset: "Réinitialiser la disposition".into(),
            close: "Fermer".into(),
            roll: "Enrouler".into(),
            unroll: "Dérouler".into(),
            maximize: "Agrandir".into(),
            restore: "Restaurer".into(),
            reopen: "Panneaux fermés".into(),
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Frame results
// ═════════════════════════════════════════════════════════════════════════════

/// What the `content` closure is asked to paint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockSlot<'a> {
    /// The centre (the web's `children`).
    Viewport,
    /// The body of panel `id` (the active tab of a group).
    Panel(&'a str),
}

/// What happened during a frame, for the host (and the designable control's
/// events).
#[derive(Debug, Clone, PartialEq)]
pub enum DockEvent {
    /// The user made `id` the active tab (click, keyboard, reopen).
    Activated(PanelId),
    /// The user closed `id`.
    Closed(PanelId),
    /// `id` was reopened.
    Opened(PanelId),
    /// The layout changed (drop, resize, close, roll…), once per frame.
    LayoutChanged,
}

/// [`DockArea::frame`]'s report.
#[derive(Debug, Clone, Default)]
pub struct DockRun {
    /// The viewport's rectangle.
    pub viewport: Rect,
    pub events: Vec<DockEvent>,
    /// A drag, a resize or the tab menu holds the pointer.
    pub busy: bool,
}

// ═════════════════════════════════════════════════════════════════════════════
// Geometry of a frame
// ═════════════════════════════════════════════════════════════════════════════

#[derive(Debug, Clone)]
struct TabGeo {
    panel: PanelId,
    rect: Rect,
    close: Option<Rect>,
}

#[derive(Debug, Clone)]
struct GroupGeo {
    gid: String,
    side: DockSide,
    /// The group's box (the card for a docked group, the whole float).
    card: Rect,
    strip: Rect,
    tabs: Vec<TabGeo>,
    roll: Option<Rect>,
    max: Option<Rect>,
    /// The body, `None` for a rolled float.
    content: Option<Rect>,
}

#[derive(Debug, Clone, Default)]
struct Geo {
    bounds: Rect,
    viewport: Rect,
    docked: Vec<GroupGeo>,
    floats: Vec<GroupGeo>,
    col_resizers: Vec<(DockSide, Rect)>,
    row_resizers: Vec<(DockSide, usize, Rect)>,
}

impl Geo {
    fn groups(&self) -> impl Iterator<Item = &GroupGeo> {
        self.docked.iter().chain(self.floats.iter())
    }

    /// The top-most float under `(x, y)`.
    fn float_at(&self, x: f32, y: f32) -> Option<usize> {
        self.floats.iter().rposition(|g| g.card.contains(x, y))
    }
}

/// The tabs of a group flowed into rows: `(panel, x offset, width, row)` per
/// tab, the row count, and where a float's caption controls go `(x, row)`.
type TabFlow = (Vec<(PanelId, f32, f32, usize)>, usize, (f32, usize));

/// A chrome target under the pointer.
#[derive(Debug, Clone, PartialEq)]
enum Hit {
    Tab(PanelId),
    TabClose(PanelId),
    Roll(String),
    Max(String),
    ColResize(DockSide),
    RowResize(DockSide, usize),
}

/// `Guide`: one dock indicator.
#[derive(Debug, Clone, PartialEq)]
struct Guide {
    id: &'static str,
    cx: f32,
    cy: f32,
    dir: char,
    tgt: DropTarget,
    rect: Rect,
}

#[derive(Debug, Clone)]
struct Drag {
    panel: PanelId,
    start: (f32, f32),
    moved: bool,
    /// The source group's size (`dragSize`), for the float target.
    size: (f32, f32),
}

#[derive(Debug, Clone)]
enum Resize {
    Col { side: DockSide, start_x: f32, start_w: f32 },
    Row { side: DockSide, index: usize, start_y: f32, h1: f32, h2: f32 },
}

#[derive(Clone)]
struct TabMenu {
    panel: PanelId,
    at: (f32, f32),
    menu: Menu,
    /// What each row does, index-aligned with the menu's items.
    actions: Vec<Option<MenuAction>>,
}

#[derive(Clone)]
struct ReopenMenu {
    anchor: Rect,
    menu: Menu,
    ids: Vec<PanelId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuAction {
    Float,
    DockLeft,
    DockRight,
    Close,
    Reset,
}

// ═════════════════════════════════════════════════════════════════════════════
// DockArea — the controller
// ═════════════════════════════════════════════════════════════════════════════

/// `DockArea`: the layout state, persistence, drag logic and painting of a
/// viewport surrounded by dockable panels. See the module doc.
pub struct DockArea {
    /// The panel registry.
    pub panels: Vec<DockPanel>,
    pub default_arrangement: DockArrangement,
    /// `None` follows the app's theme ([`DockTheme::from_theme`]).
    pub theme: Option<DockTheme>,
    /// `viewportBg` (`#141414` when `None`, the web's default).
    pub viewport_bg: Option<D2D1_COLOR_F>,
    /// `hidden`: the viewport alone, full size (the panels are kept).
    pub hidden: bool,
    /// `moveTitle`: the tabs' tooltip (reserved; the desktop shows none yet).
    pub move_title: Option<String>,
    /// Whether the dock takes its keyboard shortcuts (Ctrl+Tab…).
    pub keyboard: bool,
    pub strings: DockStrings,
    /// When set, the layout is persisted under this key (the web's
    /// `storageKey`): `%LOCALAPPDATA%\Kubuno\layouts\<key>.json`.
    storage_key: Option<String>,
    layout: DockLayout,
    revision: u64,
    dirty_storage: bool,
    drag: Option<Drag>,
    resize: Option<Resize>,
    tab_menu: Option<TabMenu>,
    reopen_menu: Option<ReopenMenu>,
    pressed: Option<Hit>,
    prev_down: bool,
    prev_right: bool,
    /// The panel the keyboard last surfaced (drawn with a focus ring).
    focused: Option<PanelId>,
    focus_visible: bool,
    guides: Vec<Guide>,
    active_guide: Option<&'static str>,
    ghost: Option<Rect>,
    geo: Geo,
    events: Vec<DockEvent>,
    reopen_hot: bool,
    reopen_pressed: bool,
}

impl DockArea {
    /// A dock over `panels`, laid out as `default_arrangement`.
    pub fn new(panels: Vec<DockPanel>, default_arrangement: DockArrangement) -> Self {
        let mut d = Self {
            panels,
            default_arrangement,
            theme: None,
            viewport_bg: None,
            hidden: false,
            move_title: None,
            keyboard: true,
            strings: DockStrings::default(),
            storage_key: None,
            layout: DockLayout::default(),
            revision: 0,
            dirty_storage: false,
            drag: None,
            resize: None,
            tab_menu: None,
            reopen_menu: None,
            pressed: None,
            prev_down: false,
            prev_right: false,
            focused: None,
            focus_visible: false,
            guides: Vec::new(),
            active_guide: None,
            ghost: None,
            geo: Geo::default(),
            events: Vec::new(),
            reopen_hot: false,
            reopen_pressed: false,
        };
        d.layout = d.default_layout();
        d
    }

    /// Builder: the theme.
    pub fn with_theme(mut self, theme: DockTheme) -> Self {
        self.theme = Some(theme);
        self
    }

    /// Builder: the viewport's background.
    pub fn with_viewport_bg(mut self, bg: D2D1_COLOR_F) -> Self {
        self.viewport_bg = Some(bg);
        self
    }

    /// Builder: persist the layout under `key` (and load it now, if saved).
    pub fn with_storage_key(mut self, key: impl Into<String>) -> Self {
        self.set_storage_key(Some(key.into()));
        self
    }

    fn known(&self) -> Vec<&str> {
        self.panels.iter().map(|p| p.id.as_str()).collect()
    }

    fn default_layout(&self) -> DockLayout {
        reconcile(&build_default(&self.default_arrangement), &self.known())
    }

    /// The panel `id`'s registry entry.
    pub fn panel(&self, id: &str) -> Option<&DockPanel> {
        self.panels.iter().find(|p| p.id == id)
    }

    fn label(&self, id: &str) -> String {
        self.panel(id).map(|p| p.label.clone()).unwrap_or_else(|| id.to_string())
    }

    /// The current layout.
    pub fn layout(&self) -> &DockLayout {
        &self.layout
    }

    /// Bumped on every layout change — a cheap « did it change? » for a host.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Replaces the layout (reconciled against the registry).
    pub fn set_layout(&mut self, layout: DockLayout) {
        let l = reconcile(&layout, &self.known());
        self.commit(l);
    }

    /// `save`: the layout as JSON.
    pub fn save_layout(&self) -> String {
        self.layout.to_json()
    }

    /// `load`: a layout from JSON — `false` (and nothing changes) when it is
    /// not one. An EMPTY string restores the default arrangement.
    pub fn load_layout(&mut self, json: &str) -> bool {
        if json.trim().is_empty() {
            self.reset();
            return true;
        }
        match DockLayout::from_json(json) {
            Some(l) => {
                self.set_layout(l);
                true
            }
            None => false,
        }
    }

    /// Re-reads the registry (panels added or removed since): the layout is
    /// reconciled against it.
    pub fn set_panels(&mut self, panels: Vec<DockPanel>) {
        if panels != self.panels {
            self.panels = panels;
            let l = reconcile(&self.layout, &self.known());
            self.commit(l);
        }
    }

    /// The persistence key; setting one loads what was saved under it.
    pub fn set_storage_key(&mut self, key: Option<String>) {
        if self.storage_key == key {
            return;
        }
        self.storage_key = key;
        if let Some(json) = self.storage_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            if let Some(l) = DockLayout::from_json(&json) {
                self.layout = reconcile(&l, &self.known());
                self.revision += 1;
            }
        }
    }

    fn storage_path(&self) -> Option<std::path::PathBuf> {
        let key = self.storage_key.as_ref()?;
        let base = std::env::var_os("LOCALAPPDATA")?;
        let safe: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' { c } else { '_' }).collect();
        Some(std::path::PathBuf::from(base).join("Kubuno").join("layouts").join(format!("{safe}.json")))
    }

    fn persist(&mut self) {
        self.dirty_storage = false;
        let Some(path) = self.storage_path() else { return };
        if let Some(dir) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(dir) {
                tracing::warn!("dock: cannot create {}: {e}", dir.display());
                return;
            }
        }
        if let Err(e) = std::fs::write(&path, self.layout.to_json()) {
            tracing::warn!("dock: cannot save the layout to {}: {e}", path.display());
        }
    }

    fn commit(&mut self, layout: DockLayout) {
        if layout != self.layout {
            self.layout = layout;
            self.revision += 1;
            self.dirty_storage = true;
            if !self.events.contains(&DockEvent::LayoutChanged) {
                self.events.push(DockEvent::LayoutChanged);
            }
        }
    }

    /// `controller.activate`.
    pub fn activate(&mut self, id: &str) {
        let l = activate_panel(&self.layout, id);
        self.commit(l);
    }

    /// `controller.open`: surfaces or re-docks `id`.
    pub fn open(&mut self, id: &str) {
        if self.panel(id).is_none() {
            return;
        }
        let was_closed = self.layout.closed.iter().any(|p| p == id);
        let l = open_panel(&self.layout, id);
        self.commit(l);
        if was_closed {
            self.events.push(DockEvent::Opened(id.to_string()));
        }
    }

    /// `controller.close`.
    pub fn close(&mut self, id: &str) {
        if !is_docked(&self.layout, id) {
            return;
        }
        let l = close_panel(&self.layout, id);
        self.commit(l);
        self.events.push(DockEvent::Closed(id.to_string()));
        if self.focused.as_deref() == Some(id) {
            self.focused = None;
        }
    }

    /// `controller.reset`: back to the default arrangement.
    pub fn reset(&mut self) {
        let l = self.default_layout();
        self.commit(l);
    }

    /// Moves `id` to `tgt` (the programmatic twin of a drag).
    pub fn drop_panel(&mut self, id: &str, tgt: &DropTarget) {
        let l = apply_drop(&self.layout, id, tgt);
        self.commit(l);
    }

    /// The closed panels, `(id, label)` — what the web publishes to the
    /// shell's reopen control.
    pub fn closed_panels(&self) -> Vec<(PanelId, String)> {
        self.layout.closed.iter().map(|p| (p.clone(), self.label(p))).collect()
    }

    /// The active panel of the group holding the keyboard, else the first
    /// visible panel.
    pub fn focused_panel(&self) -> Option<&str> {
        self.focused.as_deref().or_else(|| self.layout.visible().first().map(|p| p.as_str()))
    }

    /// Whether a drag, a resize or a menu currently owns the pointer.
    pub fn is_busy(&self) -> bool {
        self.drag.is_some() || self.resize.is_some() || self.tab_menu.is_some() || self.reopen_menu.is_some()
    }

    // ── Geometry ────────────────────────────────────────────────────────────

    fn layout_geo(&self, c: &dyn Canvas, bounds: Rect, col: &Colors) -> Geo {
        let mut geo = Geo { bounds, ..Geo::default() };
        if self.hidden {
            geo.viewport = bounds;
            return geo;
        }
        let gap = col.gap;
        let inner = Rect::new(bounds.left + gap, bounds.top + gap, bounds.right - gap, bounds.bottom - gap);
        let mut x0 = inner.left;
        let mut x1 = inner.right;
        let lw = self.layout.left_w.unwrap_or(m::DEF_W);
        let rw = self.layout.right_w.unwrap_or(m::DEF_W);
        let mut columns: Vec<(DockSide, Rect)> = Vec::new();
        if !self.layout.left.is_empty() {
            let r = Rect::new(x0, inner.top, x0 + lw, inner.bottom);
            columns.push((DockSide::Left, r));
            geo.col_resizers.push((DockSide::Left, Rect::new(r.right, inner.top, r.right + gap, inner.bottom)));
            x0 = r.right + gap;
        }
        if !self.layout.right.is_empty() {
            let r = Rect::new(x1 - rw, inner.top, x1, inner.bottom);
            columns.push((DockSide::Right, r));
            geo.col_resizers.push((DockSide::Right, Rect::new(r.left - gap, inner.top, r.left, inner.bottom)));
            x1 = r.left - gap;
        }
        geo.viewport = Rect::new(x0, inner.top, x1.max(x0), inner.bottom);
        for (side, colr) in columns {
            let groups = self.layout.side(side);
            let n = groups.len();
            let avail = (colr.bottom - colr.top - gap * (n.saturating_sub(1)) as f32).max(0.0);
            let total: f32 = groups.iter().map(|g| g.h.unwrap_or(1.0).max(0.0001)).sum();
            let mut y = colr.top;
            for (i, g) in groups.iter().enumerate() {
                let h = if i + 1 == n { colr.bottom - y } else { avail * g.h.unwrap_or(1.0).max(0.0001) / total };
                let card = Rect::new(colr.left, y, colr.right, y + h);
                geo.docked.push(self.group_geo(c, g, side, card, false));
                y += h;
                if i + 1 < n {
                    geo.row_resizers.push((side, i, Rect::new(colr.left, y, colr.right, y + gap)));
                    y += gap;
                }
            }
        }
        for g in &self.layout.float {
            let card = self.float_rect(c, g, bounds);
            geo.floats.push(self.group_geo(c, g, DockSide::Float, card, g.rolled));
        }
        geo
    }

    /// Where a float is: its stored position (clamped inside the dock), a
    /// `DEF_W` width and its height — the whole dock when maximised, its
    /// header when rolled.
    fn float_rect(&self, c: &dyn Canvas, g: &DockGroup, b: Rect) -> Rect {
        if g.max {
            return b;
        }
        let bw = b.right - b.left;
        let bh = b.bottom - b.top;
        let w = m::DEF_W.min(bw);
        let full_h = g.h.unwrap_or(m::FLOAT_DEF_H).clamp(m::FLOAT_MIN_H, (bh * m::FLOAT_MAX_H).max(m::FLOAT_MIN_H));
        let h = if g.rolled { self.strip_height(c, g, w, true) } else { full_h.min(bh) };
        let x = (b.left + g.x.unwrap_or(bw / 2.0)).min(b.right - w).max(b.left);
        let y = (b.top + g.y.unwrap_or(bh / 3.0)).min(b.bottom - h).max(b.top);
        Rect::new(x, y, x + w, y + h)
    }

    fn tab_width(&self, c: &dyn Canvas, id: &str) -> f32 {
        let f = ui_format(c, m::TAB_FONT, 600);
        let label = c.measure(&self.label(id), &f).ceil().min(m::TAB_LABEL_MAX);
        let p = self.panel(id);
        let icon = if p.and_then(|p| p.icon).is_some() { 14.0 + m::TAB_INNER_GAP } else { 0.0 };
        let close = if p.is_none_or(|p| p.closable) { m::TAB_INNER_GAP + m::CLOSE_BTN } else { 0.0 };
        m::TAB_PAD_X * 2.0 + icon + label + close
    }

    /// The tabs flowed into rows (`flex-wrap`), as `(panel, x offset, row)`,
    /// plus the row count; a float reserves its caption controls as the
    /// flow's last item.
    fn flow_tabs(&self, c: &dyn Canvas, g: &DockGroup, width: f32, float: bool) -> TabFlow {
        let pad_l = if float { 0.0 } else { m::STRIP_PAD };
        let pad_r = if float { m::CAPTION_PAD_R } else { m::STRIP_PAD };
        let avail = (width - pad_l - pad_r).max(1.0);
        let mut out = Vec::new();
        let mut x = 0.0;
        let mut row = 0;
        for p in &g.panels {
            let w = self.tab_width(c, p).min(avail);
            if x > 0.0 && x + w > avail {
                row += 1;
                x = 0.0;
            }
            out.push((p.clone(), pad_l + x, w, row));
            x += w + m::TAB_GAP;
        }
        let mut controls = (0.0, row);
        if float {
            let cw = m::CAPTION_BTN * 2.0 + m::CAPTION_GAP + m::CAPTION_PAD_R;
            if x > 0.0 && x + cw > avail {
                row += 1;
            }
            controls = (width - pad_r - cw, row);
        }
        (out, row + 1, controls)
    }

    fn strip_height(&self, c: &dyn Canvas, g: &DockGroup, width: f32, float: bool) -> f32 {
        let (_, rows, _) = self.flow_tabs(c, g, width, float);
        let top = if float { 0.0 } else { m::STRIP_PAD };
        top + rows as f32 * m::TAB_H + (rows.saturating_sub(1)) as f32 * m::TAB_GAP + 1.0
    }

    fn group_geo(&self, c: &dyn Canvas, g: &DockGroup, side: DockSide, card: Rect, rolled: bool) -> GroupGeo {
        let float = side == DockSide::Float;
        let width = card.right - card.left;
        let (tabs, _, (cx, crow)) = self.flow_tabs(c, g, width, float);
        let top = card.top + if float { 0.0 } else { m::STRIP_PAD };
        let strip_h = self.strip_height(c, g, width, float);
        let strip = Rect::new(card.left, card.top, card.right, (card.top + strip_h).min(card.bottom));
        let tabs = tabs
            .into_iter()
            .map(|(p, x, w, row)| {
                let y = top + row as f32 * (m::TAB_H + m::TAB_GAP);
                let rect = Rect::new(card.left + x, y, card.left + x + w, y + m::TAB_H);
                let close = self.panel(&p).is_none_or(|d| d.closable).then(|| {
                    let r = rect.right - m::TAB_PAD_X;
                    let cy = (rect.top + rect.bottom) / 2.0;
                    Rect::new(r - m::CLOSE_BTN, cy - m::CLOSE_BTN / 2.0, r, cy + m::CLOSE_BTN / 2.0)
                });
                TabGeo { panel: p, rect, close }
            })
            .collect();
        let (roll, max) = if float {
            let y = top + crow as f32 * (m::TAB_H + m::TAB_GAP) + (m::TAB_H - m::CAPTION_BTN) / 2.0;
            let x = card.left + cx;
            (
                Some(Rect::new(x, y, x + m::CAPTION_BTN, y + m::CAPTION_BTN)),
                Some(Rect::new(x + m::CAPTION_BTN + m::CAPTION_GAP, y, x + m::CAPTION_BTN * 2.0 + m::CAPTION_GAP, y + m::CAPTION_BTN)),
            )
        } else {
            (None, None)
        };
        let content = (!rolled && strip.bottom < card.bottom).then(|| Rect::new(card.left, strip.bottom, card.right, card.bottom));
        GroupGeo { gid: g.id.clone(), side, card, strip, tabs, roll, max, content }
    }

    /// The chrome target under `(x, y)` — floats first (top-most last in the
    /// list, so searched backwards), then the docked groups and the resizers.
    fn hit(&self, x: f32, y: f32) -> Option<Hit> {
        let geo = &self.geo;
        let in_group = |g: &GroupGeo| -> Option<Hit> {
            if let Some(r) = g.roll.filter(|r| r.contains(x, y)) {
                let _ = r;
                return Some(Hit::Roll(g.gid.clone()));
            }
            if g.max.is_some_and(|r| r.contains(x, y)) {
                return Some(Hit::Max(g.gid.clone()));
            }
            for t in &g.tabs {
                if t.close.is_some_and(|r| r.contains(x, y)) {
                    return Some(Hit::TabClose(t.panel.clone()));
                }
                if t.rect.contains(x, y) {
                    return Some(Hit::Tab(t.panel.clone()));
                }
            }
            None
        };
        if let Some(i) = geo.float_at(x, y) {
            return in_group(&geo.floats[i]);
        }
        for g in &geo.docked {
            if g.card.contains(x, y) {
                return in_group(g);
            }
        }
        if let Some((side, _)) = geo.col_resizers.iter().find(|(_, r)| r.contains(x, y)) {
            return Some(Hit::ColResize(*side));
        }
        if let Some((side, i, _)) = geo.row_resizers.iter().find(|(_, _, r)| r.contains(x, y)) {
            return Some(Hit::RowResize(*side, *i));
        }
        None
    }

    // ── The guide diamond ───────────────────────────────────────────────────

    /// `computeGuides`: the window-edge arrows (new left / right column) and
    /// a five-way compass over the docked pane under the pointer (centre =
    /// merge as a tab, N/S = split above/below, W/E = a side column). No
    /// compass over a float.
    fn compute_guides(&self, x: f32, y: f32) -> Vec<Guide> {
        let b = self.geo.bounds;
        let bh = b.bottom - b.top;
        let cy_b = b.top + bh / 2.0;
        let left_col = Rect::new(b.left, b.top, b.left + m::DEF_W, b.bottom);
        let right_col = Rect::new(b.right - m::DEF_W, b.top, b.right, b.bottom);
        let mut out = vec![
            Guide { id: "win-w", cx: b.left + m::GUIDE_EDGE, cy: cy_b, dir: 'W', tgt: DropTarget::NewCol { side: DockSide::Left }, rect: left_col },
            Guide { id: "win-e", cx: b.right - m::GUIDE_EDGE, cy: cy_b, dir: 'E', tgt: DropTarget::NewCol { side: DockSide::Right }, rect: right_col },
        ];
        if self.geo.float_at(x, y).is_some() {
            return out;
        }
        if let Some(g) = self.geo.docked.iter().find(|g| g.card.contains(x, y)) {
            let r = g.card;
            let (cx, cy, d) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0, m::GUIDE_ARM);
            let mid = (r.top + r.bottom) / 2.0;
            let (side, gid) = (g.side, g.gid.clone());
            out.push(Guide { id: "d-c", cx, cy, dir: 'C', tgt: DropTarget::Tabs { side, gid: gid.clone() }, rect: r });
            out.push(Guide { id: "d-n", cx, cy: cy - d, dir: 'N', tgt: DropTarget::Split { side, gid: gid.clone(), at: Where::Top }, rect: Rect::new(r.left, r.top, r.right, mid) });
            out.push(Guide { id: "d-s", cx, cy: cy + d, dir: 'S', tgt: DropTarget::Split { side, gid, at: Where::Bottom }, rect: Rect::new(r.left, mid, r.right, r.bottom) });
            out.push(Guide { id: "d-w", cx: cx - d, cy, dir: 'W', tgt: DropTarget::NewCol { side: DockSide::Left }, rect: left_col });
            out.push(Guide { id: "d-e", cx: cx + d, cy, dir: 'E', tgt: DropTarget::NewCol { side: DockSide::Right }, rect: right_col });
        }
        out
    }

    /// `guideHit`: the nearest guide within `GUIDE_HIT`.
    fn guide_hit(gs: &[Guide], x: f32, y: f32) -> Option<&Guide> {
        let mut best: Option<&Guide> = None;
        let mut bd = m::GUIDE_HIT;
        for g in gs {
            let d = ((x - g.cx).powi(2) + (y - g.cy).powi(2)).sqrt();
            if d <= bd {
                bd = d;
                best = Some(g);
            }
        }
        best
    }

    /// `snapFloat`: a float's edges snap to the dock's edges and to the other
    /// floats' edges (the one being moved excluded).
    fn snap_float(&self, left: f32, top: f32, width: f32, height: f32, dragged: &str) -> (f32, f32) {
        let b = self.geo.bounds;
        let mut xs = vec![b.left, b.right];
        let mut ys = vec![b.top, b.bottom];
        for (g, geo) in self.layout.float.iter().zip(self.geo.floats.iter()) {
            if g.panels.iter().any(|p| p == dragged) {
                continue;
            }
            xs.extend([geo.card.left, geo.card.right]);
            ys.extend([geo.card.top, geo.card.bottom]);
        }
        let (mut l, mut t) = (left, top);
        for x in xs {
            if (l - x).abs() <= m::SNAP {
                l = x;
                break;
            }
            if (l + width - x).abs() <= m::SNAP {
                l = x - width;
                break;
            }
        }
        for y in ys {
            if (t - y).abs() <= m::SNAP {
                t = y;
                break;
            }
            if (t + height - y).abs() <= m::SNAP {
                t = y - height;
                break;
            }
        }
        (l, t)
    }

    /// `floatTarget`: where the panel lands when not dropped on a guide — a
    /// float under the pointer, snapped; the target is dock-relative, the
    /// ghost in client DIP.
    fn float_target(&self, drag: &Drag, x: f32, y: f32) -> (DropTarget, Rect) {
        let b = self.geo.bounds;
        let fw = m::DEF_W.min(b.right - b.left);
        let fh = drag.size.1.clamp(m::FLOAT_MIN_H, ((b.bottom - b.top) * m::FLOAT_MAX_H).max(m::FLOAT_MIN_H));
        let left = (x - fw / 2.0).max(b.left + m::FLOAT_MARGIN);
        let top = (y - m::FLOAT_GRAB_Y).max(b.top + m::FLOAT_MARGIN);
        let (l, t) = self.snap_float(left, top, fw, fh, &drag.panel);
        let l = l.min(b.right - fw).max(b.left);
        let t = t.min(b.bottom - fh).max(b.top);
        (DropTarget::Float { x: l - b.left, y: t - b.top }, Rect::new(l, t, l + fw, t + fh))
    }

    /// `floatHere`: the tab menu's « detach » — a float at the dock's centre
    /// (top-left at mid-width, a third of the height, like the web).
    fn float_here(&mut self, id: &str) {
        let b = self.geo.bounds;
        let h = self.geo.groups().find(|g| g.tabs.iter().any(|t| t.panel == id)).map(|g| g.card.bottom - g.card.top);
        let tgt = DropTarget::Float { x: (b.right - b.left) / 2.0, y: (b.bottom - b.top) / 3.0 };
        let mut l = apply_drop(&self.layout, id, &tgt);
        if let Some(g) = l.float.last_mut() {
            g.h = h.map(|h| h.clamp(m::FLOAT_MIN_H, ((b.bottom - b.top) * m::FLOAT_MAX_H).max(m::FLOAT_MIN_H)));
        }
        self.commit(l);
    }

    // ── The frame ───────────────────────────────────────────────────────────

    /// Runs one frame of the dock in `bounds`: input, chrome, and the
    /// caller's `content` for the viewport and every visible panel body (see
    /// the module doc).
    pub fn frame(
        &mut self,
        c: &dyn Canvas,
        bounds: Rect,
        f: &Frame,
        content: &mut dyn FnMut(&dyn Canvas, &Frame, DockSlot<'_>, Rect),
    ) -> DockRun {
        let col = self.theme.unwrap_or_else(|| DockTheme::from_theme(c.theme())).resolve();
        self.geo = self.layout_geo(c, bounds, &col);
        let (mx, my) = f.mouse;
        let pressed_edge = f.mouse_down && !self.prev_down;
        let released = !f.mouse_down && self.prev_down;
        let right_edge = f.right_down && !self.prev_right;
        self.prev_down = f.mouse_down;
        self.prev_right = f.right_down;

        // ── Keyboard ────────────────────────────────────────────────────────
        self.keys();

        // ── Menus own the pointer while open ────────────────────────────────
        let menu_took = self.menus_input(c, f, pressed_edge, released);

        // ── Presses ─────────────────────────────────────────────────────────
        let hot = if self.hidden || f.pointer_outside() { None } else { self.hit(mx, my) };
        if !menu_took && !self.hidden {
            if pressed_edge {
                self.focus_visible = false;
                self.pressed = hot.clone();
                match &hot {
                    Some(Hit::Tab(p)) => {
                        if f.click_count >= 2 {
                            // `onDoubleClick`: docked → float here, float → right column.
                            self.pressed = None;
                            let floating = self.layout.float.iter().any(|g| g.panels.contains(p));
                            let p = p.clone();
                            if floating {
                                self.drop_panel(&p, &DropTarget::NewCol { side: DockSide::Right });
                            } else {
                                self.float_here(&p);
                            }
                            self.drag = None;
                        } else {
                            let size = self
                                .geo
                                .groups()
                                .find(|g| g.tabs.iter().any(|t| &t.panel == p))
                                .map(|g| (g.card.right - g.card.left, g.card.bottom - g.card.top))
                                .unwrap_or((m::DEF_W, m::FLOAT_DEF_H));
                            self.drag = Some(Drag { panel: p.clone(), start: (mx, my), moved: false, size });
                        }
                    }
                    Some(Hit::ColResize(side)) => {
                        let start_w = match side {
                            DockSide::Left => self.layout.left_w,
                            _ => self.layout.right_w,
                        }
                        .unwrap_or(m::DEF_W);
                        self.resize = Some(Resize::Col { side: *side, start_x: mx, start_w });
                    }
                    Some(Hit::RowResize(side, i)) => self.start_row_resize(*side, *i, my),
                    _ => {}
                }
            }
            if right_edge {
                if let Some(Hit::Tab(p)) = &hot {
                    let p = p.clone();
                    self.open_tab_menu(&p, (mx, my));
                }
            }
        }

        // ── Drag in progress ────────────────────────────────────────────────
        if let Some(drag) = self.drag.clone() {
            if f.mouse_down {
                let moved = drag.moved || ((mx - drag.start.0).powi(2) + (my - drag.start.1).powi(2)).sqrt() > m::DRAG_SLOP;
                if let Some(d) = self.drag.as_mut() {
                    d.moved = moved;
                }
                if moved {
                    let gs = self.compute_guides(mx, my);
                    let hit = Self::guide_hit(&gs, mx, my).cloned();
                    self.active_guide = hit.as_ref().map(|g| g.id);
                    self.ghost = Some(match &hit {
                        Some(g) => g.rect,
                        None => self.float_target(&drag, mx, my).1,
                    });
                    self.guides = gs;
                } else {
                    self.ghost = None;
                    self.guides.clear();
                }
            } else {
                if !drag.moved {
                    self.user_activate(&drag.panel);
                } else {
                    let gs = self.compute_guides(mx, my);
                    let (tgt, h) = match Self::guide_hit(&gs, mx, my) {
                        Some(g) => (g.tgt.clone(), None),
                        None => {
                            let (t, r) = self.float_target(&drag, mx, my);
                            (t, Some(r.bottom - r.top))
                        }
                    };
                    let mut l = apply_drop(&self.layout, &drag.panel, &tgt);
                    if let (Some(h), Some(g)) = (h, l.float.last_mut()) {
                        g.h = Some(h);
                    }
                    self.commit(l);
                    self.events.push(DockEvent::Activated(drag.panel.clone()));
                    self.focused = Some(drag.panel.clone());
                }
                self.end_drag();
            }
        }

        // ── Resize in progress ──────────────────────────────────────────────
        if let Some(rz) = self.resize.clone() {
            if f.mouse_down {
                match rz {
                    Resize::Col { side, start_x, start_w } => {
                        let delta = mx - start_x;
                        let w = if side == DockSide::Left { start_w + delta } else { start_w - delta }.clamp(m::MIN_W, m::MAX_W);
                        let mut l = self.layout.clone();
                        if side == DockSide::Left {
                            l.left_w = Some(w);
                        } else {
                            l.right_w = Some(w);
                        }
                        self.commit(l);
                    }
                    Resize::Row { side, index, start_y, h1, h2 } => {
                        let total = h1 + h2;
                        let n1 = (h1 + (my - start_y)).min(total - m::MIN_H).max(m::MIN_H);
                        let mut l = self.layout.clone();
                        let arr = l.side_mut(side);
                        if let Some(g) = arr.get_mut(index) {
                            g.h = Some(n1);
                        }
                        if let Some(g) = arr.get_mut(index + 1) {
                            g.h = Some(total - n1);
                        }
                        self.commit(l);
                    }
                }
            } else {
                self.resize = None;
            }
        }

        // ── Releases on buttons ─────────────────────────────────────────────
        if released {
            if let Some(p) = self.pressed.take() {
                if hot.as_ref() == Some(&p) {
                    match p {
                        Hit::TabClose(id) => self.close(&id),
                        Hit::Roll(gid) => self.toggle_float(&gid, true),
                        Hit::Max(gid) => self.toggle_float(&gid, false),
                        _ => {}
                    }
                }
            }
        }

        // The geometry follows the layout changes of this frame.
        self.geo = self.layout_geo(c, bounds, &col);

        // ── Cursor ─────────────────────────────────────────────────────────
        match (&self.resize, &hot) {
            (Some(Resize::Col { .. }), _) | (None, Some(Hit::ColResize(_))) => host::set_cursor(Cursor::ResizeEW),
            (Some(Resize::Row { .. }), _) | (None, Some(Hit::RowResize(..))) => host::set_cursor(Cursor::ResizeNS),
            _ if self.drag.as_ref().is_some_and(|d| d.moved) => host::set_cursor(Cursor::Move),
            (None, Some(Hit::TabClose(_) | Hit::Roll(_) | Hit::Max(_))) => host::set_cursor(Cursor::Hand),
            _ => {}
        }

        // ── Paint ──────────────────────────────────────────────────────────
        self.paint(c, f, &col, hot.as_ref(), content);

        if self.dirty_storage && self.drag.is_none() && self.resize.is_none() {
            self.persist();
        }
        let busy = self.is_busy();
        DockRun { viewport: self.geo.viewport, events: std::mem::take(&mut self.events), busy }
    }

    fn end_drag(&mut self) {
        self.drag = None;
        self.ghost = None;
        self.guides.clear();
        self.active_guide = None;
    }

    fn user_activate(&mut self, id: &str) {
        let l = activate_panel(&self.layout, id);
        self.commit(l);
        self.focused = Some(id.to_string());
        self.events.push(DockEvent::Activated(id.to_string()));
    }

    /// `startRowResize`: freezes every group's weight to its pixel height so
    /// only the dragged pair changes.
    fn start_row_resize(&mut self, side: DockSide, index: usize, y: f32) {
        let heights: Vec<f32> = self.geo.docked.iter().filter(|g| g.side == side).map(|g| g.card.bottom - g.card.top).collect();
        let mut l = self.layout.clone();
        for (g, h) in l.side_mut(side).iter_mut().zip(heights.iter()) {
            g.h = Some(*h);
        }
        self.commit(l);
        let h1 = heights.get(index).copied().unwrap_or(0.0);
        let h2 = heights.get(index + 1).copied().unwrap_or(0.0);
        self.resize = Some(Resize::Row { side, index, start_y: y, h1, h2 });
    }

    /// `toggleRoll` / `toggleMax`.
    fn toggle_float(&mut self, gid: &str, roll: bool) {
        let mut l = self.layout.clone();
        for g in l.float.iter_mut().filter(|g| g.id == gid) {
            if roll {
                g.rolled = !g.rolled;
            } else {
                g.max = !g.max;
            }
        }
        self.commit(l);
    }

    fn keys(&mut self) {
        // Escape cancels a drag or a resize (the drag leaves the layout as it
        // was; a resize snaps back to where it started).
        if (self.drag.is_some() || self.resize.is_some()) && host::take_key(vk::ESCAPE, Modifiers::NONE) > 0 {
            if let Some(Resize::Col { side, start_w, .. }) = self.resize.clone() {
                let mut l = self.layout.clone();
                match side {
                    DockSide::Left => l.left_w = Some(start_w),
                    _ => l.right_w = Some(start_w),
                }
                self.commit(l);
            }
            if let Some(Resize::Row { side, index, h1, h2, .. }) = self.resize.clone() {
                let mut l = self.layout.clone();
                let arr = l.side_mut(side);
                if let Some(g) = arr.get_mut(index) {
                    g.h = Some(h1);
                }
                if let Some(g) = arr.get_mut(index + 1) {
                    g.h = Some(h2);
                }
                self.commit(l);
            }
            self.end_drag();
            self.resize = None;
            self.pressed = None;
        }
        if !self.keyboard || self.hidden || self.tab_menu.is_some() || self.reopen_menu.is_some() {
            return;
        }
        let fwd = host::take_key(vk::TAB, Modifiers::CTRL);
        let back = host::take_key(vk::TAB, Modifiers::CTRL_SHIFT);
        let steps = fwd as isize - back as isize;
        if steps != 0 {
            self.cycle(steps);
        }
        let menu = host::take_key(vk::APPS, Modifiers::NONE) + host::take_key(vk::F10, Modifiers::SHIFT);
        if menu > 0 {
            if let Some(p) = self.focused_panel().map(str::to_string) {
                let at = self
                    .geo
                    .groups()
                    .flat_map(|g| g.tabs.iter())
                    .find(|t| t.panel == p)
                    .map(|t| (t.rect.left, t.rect.bottom))
                    .unwrap_or((self.geo.bounds.left, self.geo.bounds.top));
                self.open_tab_menu(&p, at);
                self.focus_visible = true;
            }
        }
    }

    /// Ctrl+Tab: surfaces the next panel (all placed panels, in layout order).
    fn cycle(&mut self, steps: isize) {
        let all: Vec<PanelId> = self.layout.placed().into_iter().cloned().collect();
        if all.is_empty() {
            return;
        }
        let n = all.len() as isize;
        let cur = self.focused.as_ref().and_then(|p| all.iter().position(|q| q == p)).map(|i| i as isize);
        let next = match cur {
            Some(i) => ((i + steps) % n + n) % n,
            None => {
                if steps > 0 {
                    0
                } else {
                    n - 1
                }
            }
        } as usize;
        let id = all[next].clone();
        let l = open_panel(&self.layout, &id);
        self.commit(l);
        self.focused = Some(id.clone());
        self.focus_visible = true;
        self.events.push(DockEvent::Activated(id));
    }

    // ── Menus ───────────────────────────────────────────────────────────────

    fn open_tab_menu(&mut self, panel: &str, at: (f32, f32)) {
        let closable = self.panel(panel).is_none_or(|p| p.closable);
        let s = &self.strings;
        let mut items: Vec<StripItem> = vec![lists::section(self.label(panel)), lists::separator()];
        let mut actions = vec![None, None];
        let mut push = |item: StripItem, a: Option<MenuAction>| {
            items.push(item);
            actions.push(a);
        };
        push(MenuEntry::new(s.float.clone()).build(), Some(MenuAction::Float));
        push(MenuEntry::new(s.dock_left.clone()).build(), Some(MenuAction::DockLeft));
        push(MenuEntry::new(s.dock_right.clone()).build(), Some(MenuAction::DockRight));
        if closable {
            push(lists::separator(), None);
            push(MenuEntry::new(s.close_panel.clone()).danger().build(), Some(MenuAction::Close));
        }
        push(lists::separator(), None);
        push(MenuEntry::new(s.reset.clone()).build(), Some(MenuAction::Reset));
        let mut menu = Menu::with_items(items);
        if self.focus_visible {
            menu.hot_index = menu.next_actionable(None, true);
        }
        self.tab_menu = Some(TabMenu { panel: panel.to_string(), at, menu, actions });
        self.end_drag();
        self.pressed = None;
    }

    fn run_menu_action(&mut self, panel: &str, a: MenuAction) {
        match a {
            MenuAction::Float => self.float_here(panel),
            MenuAction::DockLeft => self.drop_panel(panel, &DropTarget::NewCol { side: DockSide::Left }),
            MenuAction::DockRight => self.drop_panel(panel, &DropTarget::NewCol { side: DockSide::Right }),
            MenuAction::Close => self.close(panel),
            MenuAction::Reset => self.reset(),
        }
    }

    /// The menu's panel rectangle (placed at its anchor point, kept inside
    /// the screen area).
    fn menu_panel(c: &dyn Canvas, menu: &mut Menu, at: (f32, f32), area: Rect, min_w: f32) -> Rect {
        menu.viewport = Some(area);
        let want = menu.measure(c);
        let w = want.width.max(min_w);
        let e = lists::VIEWPORT_EDGE;
        let x = at.0.min(area.right - e - w).max(area.left + e);
        let y = if at.1 + want.height > area.bottom - e { (at.1 - want.height).max(area.top + e) } else { at.1 };
        Rect::new(x, y, x + w, y + want.height)
    }

    /// Hover, clicks and keys of an open menu; `true` when the menu took the
    /// pointer this frame (a press outside closes it and is swallowed, like
    /// the web's click-outside).
    fn menus_input(&mut self, c: &dyn Canvas, f: &Frame, pressed: bool, released: bool) -> bool {
        let area = f.screen_area();
        let (mx, my) = f.mouse;
        if f.dismiss {
            self.tab_menu = None;
            self.reopen_menu = None;
        }
        if let Some(mut tm) = self.tab_menu.take() {
            let panel = Self::menu_panel(c, &mut tm.menu, tm.at, area, 200.0);
            let mut chosen: Option<usize> = None;
            let mut close = false;
            if panel.contains(mx, my) {
                if let Some(i) = tm.menu.item_at(panel, mx, my).filter(|&i| tm.menu.is_actionable(i)) {
                    tm.menu.hot_index = Some(i);
                    if released {
                        chosen = Some(i);
                    }
                }
            } else if pressed {
                close = true;
            }
            for k in menu_keys() {
                match tm.menu.navigate(k) {
                    MenuOutcome::Chosen { index, .. } => chosen = Some(index),
                    MenuOutcome::Close => close = true,
                    _ => {}
                }
            }
            if let Some(i) = chosen {
                if let Some(Some(a)) = tm.actions.get(i).copied() {
                    let p = tm.panel.clone();
                    self.run_menu_action(&p, a);
                }
                return true;
            }
            if !close {
                self.tab_menu = Some(tm);
            }
            return true;
        }
        false
    }

    fn show_menus(&mut self, c: &dyn Canvas, area: Rect) {
        if let Some(tm) = self.tab_menu.as_mut() {
            let panel = Self::menu_panel(c, &mut tm.menu, tm.at, area, 200.0);
            show_menu(c, &tm.menu, panel, area);
        }
    }

    /// The reopen control — the web's right-rail button: a round button with
    /// the `PanelRightOpen` icon and a count badge, painted at `rect` (a 40
    /// DIP square is the rail's size) while panels are closed; its click
    /// lists the closed panels in a menu, a row reopens one. Returns `true`
    /// when it painted (something is closed).
    pub fn paint_reopen_button(&mut self, c: &dyn Canvas, rect: Rect, f: &Frame) -> bool {
        let (mx, my) = f.mouse;
        let hot = !f.pointer_outside() && rect.contains(mx, my);
        let pressed_edge = f.mouse_down && !self.reopen_pressed;
        let released = !f.mouse_down && self.reopen_pressed;
        self.reopen_pressed = f.mouse_down;
        let area = f.screen_area();
        if f.dismiss {
            self.reopen_menu = None;
        }
        // The open menu (handled here, in the button's own coordinates: the
        // button may live in another pane than the dock).
        if let Some(mut rm) = self.reopen_menu.take() {
            rm.anchor = rect;
            let at = ((rm.anchor.left - 208.0).max(area.left + 8.0), rm.anchor.top);
            let panel = Self::menu_panel(c, &mut rm.menu, at, area, 200.0);
            let mut chosen: Option<usize> = None;
            let mut close = false;
            if panel.contains(mx, my) {
                if let Some(i) = rm.menu.item_at(panel, mx, my).filter(|&i| rm.menu.is_actionable(i)) {
                    rm.menu.hot_index = Some(i);
                    if released {
                        chosen = Some(i);
                    }
                }
            } else if pressed_edge && !hot {
                close = true;
            }
            for k in menu_keys() {
                match rm.menu.navigate(k) {
                    MenuOutcome::Chosen { index, .. } => chosen = Some(index),
                    MenuOutcome::Close => close = true,
                    _ => {}
                }
            }
            if let Some(id) = chosen.and_then(|i| rm.ids.get(i).cloned()) {
                self.user_open(&id);
            } else if !close {
                self.reopen_menu = Some(rm);
            }
        }
        let closed = self.closed_panels();
        if closed.is_empty() {
            self.reopen_menu = None;
            return false;
        }
        if hot && pressed_edge {
            if self.reopen_menu.is_some() {
                self.reopen_menu = None;
            } else {
                let items: Vec<StripItem> = closed.iter().map(|(_, l)| MenuEntry::new(l.clone()).build()).collect();
                let menu = Menu::with_items(items);
                self.reopen_menu = Some(ReopenMenu { anchor: rect, menu, ids: closed.iter().map(|(i, _)| i.clone()).collect() });
            }
        }
        if hot {
            host::set_cursor(Cursor::Hand);
        }
        self.reopen_hot = hot;
        let t = c.theme();
        let r = (rect.right - rect.left).min(rect.bottom - rect.top) / 2.0;
        if hot || self.reopen_menu.is_some() {
            c.fill_rounded(&rect, r, &t.surface_2);
        }
        let ink = if hot { t.text_primary } else { t.text_secondary };
        c.vector_icon("PanelRightOpen", &rect, m::REOPEN_ICON, &ink);
        let b = Rect::new(rect.right - 4.0 - m::REOPEN_BADGE, rect.top + 4.0, rect.right - 4.0, rect.top + 4.0 + m::REOPEN_BADGE);
        c.fill_rounded(&b, m::REOPEN_BADGE / 2.0, &t.accent);
        c.text(&closed.len().to_string(), &b, &ui_format(c, 10.0, 600), &t.accent_foreground, true);
        if let Some(rm) = self.reopen_menu.as_mut() {
            let area = f.screen_area();
            let at = ((rm.anchor.left - 208.0).max(area.left + 8.0), rm.anchor.top);
            let panel = Self::menu_panel(c, &mut rm.menu, at, area, 200.0);
            show_menu(c, &rm.menu, panel, area);
        }
        true
    }

    // ── Painting ────────────────────────────────────────────────────────────

    fn paint(&mut self, c: &dyn Canvas, f: &Frame, col: &Colors, hot: Option<&Hit>, content: &mut dyn FnMut(&dyn Canvas, &Frame, DockSlot<'_>, Rect)) {
        let geo = self.geo.clone();
        let busy = self.is_busy();
        let (mx, my) = f.mouse;
        let top_float = geo.float_at(mx, my);
        // The content under the pointer gets it only when nothing sits above.
        let masked = Frame {
            mouse: (host::POINTER_AWAY, host::POINTER_AWAY),
            mouse_down: false,
            right_down: false,
            middle_down: false,
            wheel: (0.0, 0.0),
            click_count: 0,
            ..*f
        };
        let frame_for = |surface: Option<usize>| -> Frame {
            if busy || surface != top_float {
                masked
            } else {
                *f
            }
        };
        let viewport_bg = self.viewport_bg.unwrap_or(fixed::VIEWPORT);
        // A panel is ONE live instance: never paint its body twice in a frame.
        let mut painted: Vec<String> = Vec::new();

        // Ground + viewport card.
        if self.hidden {
            c.push_clip(&geo.viewport);
            c.fill_rounded(&geo.viewport, 0.0, &viewport_bg);
            content(c, &frame_for(None), DockSlot::Viewport, geo.viewport);
            c.pop_clip();
            return;
        }
        c.fill_rounded(&geo.bounds, 0.0, &col.ground);
        let vr = geo.viewport;
        if vr.right > vr.left && vr.bottom > vr.top {
            c.draw_shadow(&vr, col.radius, &CARD_SHADOW, SHADOW_INK);
            c.fill_rounded(&vr, col.radius, &viewport_bg);
            c.push_clip_rounded(&vr, col.radius);
            content(c, &frame_for(None), DockSlot::Viewport, vr);
            c.pop_clip_rounded();
            c.stroke_rounded(&vr, col.radius, &col.border);
        }

        // Docked groups.
        for g in &geo.docked {
            c.draw_shadow(&g.card, col.radius, &CARD_SHADOW, SHADOW_INK);
            c.fill_rounded(&g.card, col.radius, &col.panel);
            c.push_clip_rounded(&g.card, col.radius);
            self.paint_group(c, col, g, hot);
            if let Some(body) = g.content {
                let active = self.active_of(&g.gid);
                if let Some(a) = active.filter(|a| !painted.contains(a)) {
                    painted.push(a.clone());
                    c.push_clip(&body);
                    content(c, &frame_for(None), DockSlot::Panel(&a), body);
                    c.pop_clip();
                }
            }
            c.pop_clip_rounded();
            c.stroke_rounded(&g.card, col.radius, &col.border);
        }

        // Resizers: revealed on hover (and while dragged).
        for (side, r) in &geo.col_resizers {
            let on = matches!(hot, Some(Hit::ColResize(s)) if s == side) || matches!(&self.resize, Some(Resize::Col { side: s, .. }) if s == side);
            if on {
                paint_grip(c, col, *r, true);
            }
        }
        for (side, i, r) in &geo.row_resizers {
            let on = matches!(hot, Some(Hit::RowResize(s, j)) if s == side && j == i)
                || matches!(&self.resize, Some(Resize::Row { side: s, index, .. }) if s == side && index == i);
            if on {
                paint_grip(c, col, *r, false);
            }
        }

        // Floats, in z-order.
        let frad = col.radius + 2.0;
        for (i, g) in geo.floats.iter().enumerate() {
            c.draw_shadow(&g.card, frad, &FLOAT_SHADOW, SHADOW_INK);
            c.fill_rounded(&g.card, frad, &col.panel);
            c.push_clip_rounded(&g.card, frad);
            self.paint_group(c, col, g, hot);
            if let Some(body) = g.content {
                if let Some(a) = self.active_of(&g.gid).filter(|a| !painted.contains(a)) {
                    painted.push(a.clone());
                    c.push_clip(&body);
                    content(c, &frame_for(Some(i)), DockSlot::Panel(&a), body);
                    c.pop_clip();
                }
            }
            c.pop_clip_rounded();
            c.stroke_rounded(&g.card, frad, &col.border);
        }

        // The ghost, then the guides.
        if let Some(gr) = self.ghost {
            c.fill_rounded(&gr, m::GHOST_RADIUS, &fixed::GHOST_FILL);
            let inset = Rect::new(gr.left + 1.0, gr.top + 1.0, gr.right - 1.0, gr.bottom - 1.0);
            c.stroke_rounded_w(&inset, m::GHOST_RADIUS, &fixed::GHOST_BORDER, m::GHOST_BORDER);
        }
        if self.drag.as_ref().is_some_and(|d| d.moved) {
            for g in &self.guides {
                paint_guide(c, g, self.active_guide == Some(g.id));
            }
        }
        self.show_menus(c, f.screen_area());
    }

    fn active_of(&self, gid: &str) -> Option<String> {
        SIDES.iter().flat_map(|s| self.layout.side(*s).iter()).find(|g| g.id == gid).map(|g| g.active.clone())
    }

    fn paint_group(&self, c: &dyn Canvas, col: &Colors, g: &GroupGeo, hot: Option<&Hit>) {
        let float = g.side == DockSide::Float;
        if float {
            c.fill_rounded(&g.strip, 0.0, &col.float_strip);
        }
        let rule = Rect::new(g.strip.left, g.strip.bottom - 1.0, g.strip.right, g.strip.bottom);
        c.fill_rounded(&rule, 0.0, &col.border);
        let active = self.active_of(&g.gid).unwrap_or_default();
        let r = (col.radius - 4.0).max(6.0);
        for t in &g.tabs {
            let is_active = t.panel == active;
            let tab_hot = matches!(hot, Some(Hit::Tab(p) | Hit::TabClose(p)) if *p == t.panel);
            if is_active {
                c.fill_top_rounded(&t.rect, r, &col.tab_active_bg);
            }
            let fg = if is_active { col.accent } else { col.text_dim };
            let mut x = t.rect.left + m::TAB_PAD_X;
            if let Some(icon) = self.panel(&t.panel).and_then(|p| p.icon) {
                let ir = Rect::new(x, t.rect.top, x + 14.0, t.rect.bottom);
                c.vector_icon(icon, &ir, 14.0, &fg);
                x += 14.0 + m::TAB_INNER_GAP;
            }
            // The label box reaches 3 DIP into the gap: the painter snaps text boxes to the pixel
            // grid, which can shave a fraction of a DIP and ellipsize a label that fits (the tab
            // itself keeps the web's exact width).
            let right = t.close.map(|cr| cr.left - m::TAB_INNER_GAP + 3.0).unwrap_or(t.rect.right - m::TAB_PAD_X + 3.0);
            let fmt = ui_format(c, m::TAB_FONT, if is_active { 600 } else { 500 });
            c.text_ellipsis(&self.label(&t.panel), &Rect::new(x, t.rect.top, right.max(x), t.rect.bottom), &fmt, &fg);
            if let Some(cr) = t.close {
                let close_hot = matches!(hot, Some(Hit::TabClose(p)) if *p == t.panel);
                if is_active || tab_hot {
                    let alpha = if close_hot { 1.0 } else { 0.7 };
                    if close_hot {
                        c.fill_rounded(&cr, m::CLOSE_BTN / 2.0, &col.hover);
                    }
                    let ink = D2D1_COLOR_F { a: fg.a * alpha, ..fg };
                    c.vector_icon("X", &cr, m::CLOSE_ICON, &ink);
                }
            }
            if is_active {
                let u = Rect::new(t.rect.left + m::UNDERLINE_INSET, t.rect.bottom - m::UNDERLINE_H, t.rect.right - m::UNDERLINE_INSET, t.rect.bottom);
                c.fill_top_rounded(&u, m::UNDERLINE_H, &col.accent);
            }
            if self.focus_visible && self.focused.as_deref() == Some(t.panel.as_str()) {
                let fr = Rect::new(t.rect.left + 1.0, t.rect.top + 1.0, t.rect.right - 1.0, t.rect.bottom - 1.0);
                c.stroke_rounded_w(&fr, r, &col.accent, 2.0);
            }
        }
        if float {
            let rolled = SIDES.iter().flat_map(|s| self.layout.side(*s).iter()).find(|x| x.id == g.gid).map(|x| (x.rolled, x.max)).unwrap_or((false, false));
            for (rect, which) in [(g.roll, 0), (g.max, 1)] {
                let Some(rect) = rect else { continue };
                let is_hot = matches!((hot, which), (Some(Hit::Roll(id)), 0) | (Some(Hit::Max(id)), 1) if *id == g.gid);
                if is_hot {
                    c.fill_rounded(&rect, m::CAPTION_BTN / 2.0, &col.hover);
                }
                let ink = D2D1_COLOR_F { a: col.text_dim.a * if is_hot { 1.0 } else { 0.7 }, ..col.text_dim };
                match which {
                    0 => c.vector_icon(if rolled.0 { "ChevronDown" } else { "ChevronUp" }, &rect, 14.0, &ink),
                    _ => c.vector_icon(if rolled.1 { "Copy" } else { "Square" }, &rect, 12.0, &ink),
                }
            }
        }
    }

    // ── Accessibility ───────────────────────────────────────────────────────

    /// The dock's accessibility nodes, from the last frame's geometry: one
    /// `TabList` per group, a `Tab` per panel (its `checked` = active), a
    /// `TabPanel` over each visible body. Ids are `base + n`; pass the
    /// element's own node as `parent`.
    pub fn access_nodes(&self, base: u64, parent: Option<u64>) -> Vec<access::AccessNode> {
        let mut out = Vec::new();
        let mut n = base;
        let bounds = |r: Rect| (r.left, r.top, r.right, r.bottom);
        for g in self.geo.groups() {
            n = n.wrapping_add(1);
            let list = n;
            out.push(access::AccessNode {
                id: list,
                parent,
                role: access::AccessRole::TabList,
                name: String::new(),
                bounds: bounds(g.strip),
                ..access::AccessNode::default()
            });
            let active = self.active_of(&g.gid).unwrap_or_default();
            for t in &g.tabs {
                n = n.wrapping_add(1);
                out.push(access::AccessNode {
                    id: n,
                    parent: Some(list),
                    role: access::AccessRole::Tab,
                    name: self.label(&t.panel),
                    bounds: bounds(t.rect),
                    checked: Some(t.panel == active),
                    clickable: true,
                    focusable: true,
                    ..access::AccessNode::default()
                });
            }
            if let Some(body) = g.content {
                n = n.wrapping_add(1);
                out.push(access::AccessNode {
                    id: n,
                    parent,
                    role: access::AccessRole::TabPanel,
                    name: self.label(&active),
                    bounds: bounds(body),
                    ..access::AccessNode::default()
                });
            }
        }
        out
    }

    /// The panel whose tab is access node `id` (from [`Self::access_nodes`]
    /// with the same `base`) — what a host activates on a UIA « Invoke ».
    pub fn access_panel(&self, base: u64, id: u64) -> Option<PanelId> {
        let mut n = base;
        for g in self.geo.groups() {
            n = n.wrapping_add(1);
            for t in &g.tabs {
                n = n.wrapping_add(1);
                if n == id {
                    return Some(t.panel.clone());
                }
            }
            if g.content.is_some() {
                n = n.wrapping_add(1);
            }
        }
        None
    }

    /// Activates `id` as the user would (reports [`DockEvent::Activated`]).
    pub fn user_open(&mut self, id: &str) {
        self.open(id);
        self.focused = Some(id.to_string());
        self.events.push(DockEvent::Activated(id.to_string()));
    }
}

/// The keys an open menu takes this frame.
fn menu_keys() -> Vec<MenuKey> {
    host::consume(|e| {
        matches!(e, InputEvent::Key { vk: code, down: true, mods, .. }
            if mods.is_none() && [vk::UP, vk::DOWN, vk::HOME, vk::END, vk::ENTER, vk::SPACE, vk::ESCAPE, vk::LEFT, vk::RIGHT].contains(code))
    })
    .into_iter()
    .filter_map(|e| match e {
        InputEvent::Key { vk: code, .. } => Some(match code {
            vk::UP => MenuKey::Up,
            vk::DOWN => MenuKey::Down,
            vk::HOME => MenuKey::Home,
            vk::END => MenuKey::End,
            vk::ENTER => MenuKey::Enter,
            vk::SPACE => MenuKey::Space,
            vk::LEFT => MenuKey::Left,
            vk::RIGHT => MenuKey::Right,
            _ => MenuKey::Escape,
        }),
        _ => None,
    })
    .collect()
}

/// Shows `menu` at `panel` in an interactive floating surface.
fn show_menu(c: &dyn Canvas, menu: &Menu, panel: Rect, area: Rect) {
    let pb = menu.paint_bounds(c, panel);
    let mut snap = menu.clone();
    snap.viewport = Some(Rect::new(area.left - pb.left, area.top - pb.top, area.right - pb.left, area.bottom - pb.top));
    let local = Rect::new(panel.left - pb.left, panel.top - pb.top, panel.right - pb.left, panel.bottom - pb.top);
    host::popup(pb, move |cv| snap.paint(cv, local, WidgetState::REST));
}

/// The house resize handle: a 5 DIP hairline and a grip pill.
fn paint_grip(c: &dyn Canvas, col: &Colors, r: Rect, vertical: bool) {
    let (cx, cy) = ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0);
    let half = m::GRIP_LINE / 2.0;
    let line = if vertical { Rect::new(cx - half, r.top, cx + half, r.bottom) } else { Rect::new(r.left, cy - half, r.right, cy + half) };
    c.fill_rounded(&line, half, &col.grip_line);
    let (w, h) = if vertical { (m::GRIP_SHORT, m::GRIP_LONG) } else { (m::GRIP_LONG, m::GRIP_SHORT) };
    let pill = Rect::new(cx - w / 2.0, cy - h / 2.0, cx + w / 2.0, cy + h / 2.0);
    c.draw_shadow(&pill, w.min(h) / 2.0, &GUIDE_SHADOW, SHADOW_INK);
    c.fill_rounded(&pill, w.min(h) / 2.0, &col.tab_active_bg);
    c.stroke_rounded(&pill, w.min(h) / 2.0, &col.grip_border);
    c.vector_icon(if vertical { "GripVertical" } else { "GripHorizontal" }, &pill, m::GRIP_ICON, &col.accent);
}

/// One guide of the diamond.
fn paint_guide(c: &dyn Canvas, g: &Guide, active: bool) {
    let s = if active { m::GUIDE * m::GUIDE_ACTIVE_SCALE } else { m::GUIDE };
    let r = Rect::new(g.cx - s / 2.0, g.cy - s / 2.0, g.cx + s / 2.0, g.cy + s / 2.0);
    let (bg, ink, border) = if active { (fixed::GUIDE_ACTIVE, fixed::WHITE, fixed::GUIDE_ACTIVE) } else { (fixed::GUIDE_BG, fixed::GUIDE_INK, fixed::GUIDE_BORDER) };
    c.draw_shadow(&r, m::GUIDE_RADIUS, &GUIDE_SHADOW, SHADOW_INK);
    c.fill_rounded(&r, m::GUIDE_RADIUS, &bg);
    c.stroke_rounded(&r, m::GUIDE_RADIUS, &border);
    let icon = match g.dir {
        'N' => "ChevronUp",
        'S' => "ChevronDown",
        'W' => "ChevronLeft",
        'E' => "ChevronRight",
        _ => "",
    };
    if icon.is_empty() {
        // `<Square size={13} fill=…/>`: a filled rounded square.
        let q = 13.0 * 18.0 / 24.0;
        let sq = Rect::new(g.cx - q / 2.0, g.cy - q / 2.0, g.cx + q / 2.0, g.cy + q / 2.0);
        c.fill_rounded(&sq, 2.0, &ink);
    } else {
        c.vector_icon(icon, &r, 16.0, &ink);
    }
}

#[cfg(test)]
mod tests {
    //! The layout transforms, checked against the web's own behaviour
    //! (`Dock.tsx`): tabs merge, split, new column, float, close / reopen,
    //! reconcile, and the JSON round trip.

    use super::*;

    fn ids(v: &[&str]) -> Vec<PanelId> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn sample() -> DockLayout {
        build_default(&DockArrangement::new().left(&["a"]).right(&["b"]).right(&["c", "d"]))
    }

    fn panels(side: &[DockGroup]) -> Vec<Vec<PanelId>> {
        side.iter().map(|g| g.panels.clone()).collect()
    }

    #[test]
    fn default_arrangement_builds_one_group_per_list() {
        let l = sample();
        assert_eq!(panels(&l.left), vec![ids(&["a"])]);
        assert_eq!(panels(&l.right), vec![ids(&["b"]), ids(&["c", "d"])]);
        assert_eq!(l.right[1].active, "c");
        assert_eq!((l.left_w, l.right_w), (Some(m::DEF_W), Some(m::DEF_W)));
    }

    #[test]
    fn tabs_merge_into_a_group_and_become_active() {
        let l = sample();
        let gid = l.right[1].id.clone();
        let l = apply_drop(&l, "a", &DropTarget::Tabs { side: DockSide::Right, gid });
        assert!(l.left.is_empty(), "the emptied group is dropped");
        assert_eq!(panels(&l.right), vec![ids(&["b"]), ids(&["c", "d", "a"])]);
        assert_eq!(l.right[1].active, "a");
    }

    #[test]
    fn split_inserts_above_or_below() {
        let l = sample();
        let gid = l.right[0].id.clone();
        let top = apply_drop(&l, "a", &DropTarget::Split { side: DockSide::Right, gid: gid.clone(), at: Where::Top });
        assert_eq!(panels(&top.right), vec![ids(&["a"]), ids(&["b"]), ids(&["c", "d"])]);
        let bottom = apply_drop(&l, "a", &DropTarget::Split { side: DockSide::Right, gid, at: Where::Bottom });
        assert_eq!(panels(&bottom.right), vec![ids(&["b"]), ids(&["a"]), ids(&["c", "d"])]);
    }

    #[test]
    fn newcol_appends_a_group_to_a_column() {
        let l = apply_drop(&sample(), "d", &DropTarget::NewCol { side: DockSide::Left });
        assert_eq!(panels(&l.left), vec![ids(&["a"]), ids(&["d"])]);
        assert_eq!(panels(&l.right), vec![ids(&["b"]), ids(&["c"])]);
        assert_eq!(l.right[1].active, "c", "the source group keeps a valid active tab");
    }

    #[test]
    fn float_creates_a_positioned_group() {
        let l = apply_drop(&sample(), "b", &DropTarget::Float { x: 40.0, y: 30.0 });
        assert_eq!(l.float.len(), 1);
        assert_eq!((l.float[0].x, l.float[0].y), (Some(40.0), Some(30.0)));
        assert_eq!(panels(&l.right), vec![ids(&["c", "d"])]);
    }

    #[test]
    fn dropping_into_its_own_vanished_group_lands_on_that_side() {
        let l = sample();
        let gid = l.left[0].id.clone();
        let l = apply_drop(&l, "a", &DropTarget::Tabs { side: DockSide::Left, gid });
        assert_eq!(panels(&l.left), vec![ids(&["a"])]);
        let l2 = apply_drop(&l, "a", &DropTarget::Tabs { side: DockSide::Float, gid: "gone".into() });
        assert_eq!(panels(&l2.right).last(), Some(&ids(&["a"])), "a float target falls back to the right column");
    }

    #[test]
    fn close_then_open_redocks_on_the_right() {
        let l = close_panel(&sample(), "a");
        assert!(l.left.is_empty());
        assert_eq!(l.closed, ids(&["a"]));
        let l = open_panel(&l, "a");
        assert!(l.closed.is_empty());
        assert_eq!(panels(&l.right).last(), Some(&ids(&["a"])));
    }

    #[test]
    fn opening_a_visible_panel_surfaces_it_without_a_copy() {
        let l = sample();
        let l = open_panel(&l, "d");
        assert_eq!(l.right[1].active, "d");
        assert_eq!(l.placed().iter().filter(|p| p.as_str() == "d").count(), 1);
        let mut f = apply_drop(&l, "b", &DropTarget::Float { x: 0.0, y: 0.0 });
        f.float[0].rolled = true;
        let f = open_panel(&f, "b");
        assert!(!f.float[0].rolled, "opening a rolled float unrolls it");
    }

    #[test]
    fn reconcile_drops_unknown_adds_missing_and_dedupes() {
        let mut l = sample();
        l.right[1].panels.push("a".into()); // duplicate of the left one
        l.right[0].panels.push("ghost".into());
        l.right[1].id = l.right[0].id.clone(); // duplicated group id
        l.closed = ids(&["c", "e"]); // `c` is on screen: not closed
        let r = reconcile(&l, &["a", "b", "c", "d", "e", "f"]);
        let all: Vec<&PanelId> = r.placed();
        assert_eq!(all.iter().filter(|p| p.as_str() == "a").count(), 1);
        assert!(!all.iter().any(|p| p.as_str() == "ghost"));
        assert_eq!(r.closed, ids(&["e"]), "a closed panel stays closed, an on-screen one is not closed");
        assert_eq!(panels(&r.right)[0], ids(&["f"]), "a new panel appears at the top of the right column");
        let gids: Vec<&String> = [&r.left, &r.right, &r.float].into_iter().flatten().map(|g| &g.id).collect();
        let mut dedup = gids.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(gids.len(), dedup.len(), "group ids are unique");
    }

    #[test]
    fn json_round_trip_uses_the_web_field_names() {
        let mut l = apply_drop(&sample(), "b", &DropTarget::Float { x: 12.0, y: 34.0 });
        l.float[0].rolled = true;
        l.left_w = Some(300.0);
        let json = l.to_json();
        assert!(json.contains("\"leftW\":300"), "{json}");
        assert!(json.contains("\"rolled\":true"), "{json}");
        assert!(!json.contains("\"max\""), "false flags are omitted like the web's undefined: {json}");
        let back = DockLayout::from_json(&json).expect("parses");
        assert_eq!(back, l);
        // A layout written by the web (no optional fields) reads too.
        let web = r#"{"left":[{"id":"g1","panels":["a"],"active":"a"}],"right":[],"float":[]}"#;
        let w = DockLayout::from_json(web).expect("web layout");
        assert_eq!(w.left[0].panels, ids(&["a"]));
    }

    #[test]
    fn a_restored_layout_never_collides_with_new_group_ids() {
        // A layout saved by a previous run holds a high group id; the next float must not reuse it.
        let saved = r#"{"left":[],"right":[{"id":"g90000","panels":["a"],"active":"a"}],"float":[{"id":"g90001","panels":["b"],"active":"b"}]}"#;
        let l = reconcile(&DockLayout::from_json(saved).expect("layout"), &["a", "b", "c"]);
        let l = apply_drop(&l, "c", &DropTarget::Float { x: 0.0, y: 0.0 });
        let gids: Vec<&String> = [&l.left, &l.right, &l.float].into_iter().flatten().map(|g| &g.id).collect();
        let mut dedup = gids.clone();
        dedup.sort();
        dedup.dedup();
        assert_eq!(gids.len(), dedup.len(), "{gids:?}");
    }

    #[test]
    fn activate_only_touches_the_group_holding_the_panel() {
        let l = activate_panel(&sample(), "d");
        assert_eq!(l.right[1].active, "d");
        assert_eq!(l.right[0].active, "b");
    }

    #[test]
    fn area_api_reports_events_and_reset() {
        let mut d = DockArea::new(
            vec![DockPanel::new("a", "A"), DockPanel::new("b", "B"), DockPanel::new("c", "C")],
            DockArrangement::new().left(&["a"]).right(&["b", "c"]),
        );
        let r0 = d.revision();
        d.close("a");
        assert_eq!(d.closed_panels(), vec![("a".to_string(), "A".to_string())]);
        assert!(d.revision() > r0);
        assert!(d.events.contains(&DockEvent::Closed("a".into())));
        d.open("a");
        assert!(d.events.contains(&DockEvent::Opened("a".into())));
        let saved = d.save_layout();
        d.reset();
        assert_eq!(panels(&d.layout().left), vec![ids(&["a"])]);
        assert!(d.load_layout(&saved));
        assert_eq!(panels(&d.layout().right).last(), Some(&ids(&["a"])));
        assert!(!d.load_layout("not json"));
    }
}
