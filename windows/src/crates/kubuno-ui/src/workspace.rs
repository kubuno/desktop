//! The workspace chrome — a port of the web's shared shell of every advanced
//! application (`core/frontend/src/core/shell/workspace/`: `WorkspaceShell.tsx`,
//! `MenuBar.tsx`, `WorkspaceMenuBar.tsx`, `theme.ts`), the frame PaintSharp's
//! editors and the Office apps sit in, around a [`crate::dock::DockArea`].
//!
//! # Layout (top to bottom)
//!
//! topbar (back · title icon · title + subtitle + doc info · delete · search ·
//! actions) · menu bar · options bar · (tool rail + body) · bottom bar ·
//! status bar — each optional, all data-driven: [`WorkspaceShell::frame`]
//! paints the frame and hands back the rectangles the host fills (the body,
//! the options bar, the tool rail, the topbar's action area…) and what the
//! user did ([`WorkspaceRun`]).
//!
//! # Menus
//!
//! Two bars, as on the web: the standard [`MenuBarStyle::Workspace`] bar
//! (`WorkspaceMenuBar`, 28 DIP, light or dark) and PaintSharp's compact
//! [`MenuBarStyle::Compact`] bar (`MenuBar.tsx`, 24 DIP, themed). Both open the
//! product's one menu, [`crate::lists::Menu`] (`MenuDropdown`).
//! [`build_workspace_menus`] builds the standard Fichier / Édition / Affichage /
//! Aide menus from the actions an editor wires ([`WorkspaceMenuActions`]).
//!
//! # Not ported
//!
//! The editable title (`EditableTitle`, an inline `<input>`), the chromeless
//! mode (hiding the web's global header, hosting `HeaderActions`), the search
//! overlay and the mobile two-row topbar are web-shell concerns: a desktop
//! window has its own caption, and the shell only reports a click on the
//! search button ([`WorkspaceRun::search`]).

use drive_app_controls::{Canvas, Rect};
use kubuno_controls::host::{self, vk, Cursor, Frame, InputEvent, Modifiers};
use kubuno_controls::toolstrip::StripItem;
use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::dock::{hex, ui_format};
use crate::lists::{self, Menu, MenuEntry, MenuKey, MenuOutcome};
use crate::{Widget, WidgetState};

// ═════════════════════════════════════════════════════════════════════════════
// Theme — `theme.ts`
// ═════════════════════════════════════════════════════════════════════════════

/// `WorkspaceTheme`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WorkspaceTheme {
    /// Body / canvas ground.
    pub bg: D2D1_COLOR_F,
    /// Panels (the dock), menus.
    pub panel: D2D1_COLOR_F,
    /// Tool rail, panel headers.
    pub toolbar: D2D1_COLOR_F,
    /// Menu bar + options bar.
    pub header: D2D1_COLOR_F,
    /// Active tab / hover.
    pub active: D2D1_COLOR_F,
    pub border: D2D1_COLOR_F,
    pub accent: D2D1_COLOR_F,
    pub text: D2D1_COLOR_F,
    pub text_dim: D2D1_COLOR_F,
    /// Title bar (`'#111'` when absent).
    pub topbar_bg: Option<D2D1_COLOR_F>,
    /// A COLOURED topbar: its text and icons take this colour, hovers are
    /// translucent white (the Office « coloured ribbon » look).
    pub topbar_text: Option<D2D1_COLOR_F>,
    /// Status bar (`'#111'` when absent).
    pub status_bg: Option<D2D1_COLOR_F>,
    /// Dark topbar / menus (PaintSharp) or light (Documents).
    pub dark: bool,
}

impl WorkspaceTheme {
    /// `WORKSPACE_DARK`: the PaintSharp palette (Photoshop-like).
    pub fn dark() -> Self {
        Self {
            bg: hex(0x1e1e1e),
            panel: hex(0x323232),
            toolbar: hex(0x393939),
            header: hex(0x2b2b2b),
            active: hex(0x454545),
            border: hex(0x212121),
            accent: hex(0x5a9bdc),
            text: hex(0xd6d6d6),
            text_dim: hex(0x8e8e8e),
            topbar_bg: Some(hex(0x111111)),
            topbar_text: None,
            status_bg: Some(hex(0x111111)),
            dark: true,
        }
    }

    /// `WORKSPACE_LIGHT`: the Office apps' light chrome (the `--kbn-ws-*`
    /// fallbacks).
    pub fn light() -> Self {
        Self {
            bg: hex(0xffffff),
            panel: hex(0xf8f9fa),
            toolbar: hex(0xf1f3f4),
            header: hex(0xffffff),
            active: hex(0xe8eaed),
            border: hex(0xdadce0),
            accent: hex(0x1a73e8),
            text: hex(0x202124),
            text_dim: hex(0x5f6368),
            topbar_bg: Some(hex(0xffffff)),
            topbar_text: None,
            status_bg: Some(hex(0xf8f9fa)),
            dark: false,
        }
    }

    /// `WORKSPACE_OFFICE`: light, with the blue « coloured ribbon » topbar.
    pub fn office() -> Self {
        Self { active: hex(0xe8f0fe), topbar_bg: Some(hex(0x1557b0)), topbar_text: Some(hex(0xffffff)), ..Self::light() }
    }

    /// The palette for the app's current mode.
    pub fn for_mode(dark: bool) -> Self {
        if dark {
            Self::dark()
        } else {
            Self::light()
        }
    }
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

fn fill(c: &dyn Canvas, r: Rect, colour: D2D1_COLOR_F) {
    c.fill_rounded(&r, 0.0, &colour);
}

// ═════════════════════════════════════════════════════════════════════════════
// Menu model
// ═════════════════════════════════════════════════════════════════════════════

/// A menu row — the web's rich `MenuItem` (action / separator / submenu).
#[derive(Debug, Clone, PartialEq)]
pub enum WsMenuItem {
    Action { id: String, label: String, shortcut: Option<String>, enabled: bool, checked: bool, danger: bool },
    Separator,
    Submenu { label: String, items: Vec<WsMenuItem> },
}

impl WsMenuItem {
    /// An enabled action.
    pub fn action(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::Action { id: id.into(), label: label.into(), shortcut: None, enabled: true, checked: false, danger: false }
    }

    /// Builder: the shortcut text (`"Ctrl+S"`).
    pub fn shortcut(mut self, s: impl Into<String>) -> Self {
        if let Self::Action { shortcut, .. } = &mut self {
            *shortcut = Some(s.into());
        }
        self
    }

    /// Builder: enabled or greyed.
    pub fn enabled(mut self, on: bool) -> Self {
        if let Self::Action { enabled, .. } = &mut self {
            *enabled = on;
        }
        self
    }

    /// Builder: a tick in the icon cell.
    pub fn checked(mut self, on: bool) -> Self {
        if let Self::Action { checked, .. } = &mut self {
            *checked = on;
        }
        self
    }

    fn strip(&self) -> StripItem {
        match self {
            Self::Action { label, shortcut, enabled, checked, danger, .. } => {
                let mut e = MenuEntry::new(label.clone()).enabled(*enabled).checked(*checked);
                if let Some(s) = shortcut {
                    e = e.shortcut_text(s.clone());
                }
                if *danger {
                    e = e.danger();
                }
                e.build()
            }
            Self::Separator => lists::separator(),
            Self::Submenu { label, items } => MenuEntry::new(label.clone()).submenu(items.iter().map(Self::strip).collect()).build(),
        }
    }
}

/// `WsMenu`: a top-level menu of the bar.
#[derive(Debug, Clone, PartialEq)]
pub struct WsMenu {
    pub label: String,
    pub items: Vec<WsMenuItem>,
}

impl WsMenu {
    pub fn new(label: impl Into<String>, items: Vec<WsMenuItem>) -> Self {
        Self { label: label.into(), items }
    }

    fn menu(&self) -> Menu {
        Menu::with_items(self.items.iter().map(WsMenuItem::strip).collect())
    }

    /// The id of row `index` (or of row `sub` of its submenu).
    fn id_at(&self, index: usize, sub: Option<usize>) -> Option<String> {
        let row = self.items.get(index)?;
        match (row, sub) {
            (WsMenuItem::Action { id, .. }, None) => Some(id.clone()),
            (WsMenuItem::Submenu { items, .. }, Some(j)) => match items.get(j)? {
                WsMenuItem::Action { id, .. } => Some(id.clone()),
                _ => None,
            },
            _ => None,
        }
    }
}

/// `WorkspaceMenuActions`: what an editor wires; a missing one greys its row.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WorkspaceMenuActions {
    pub new: bool,
    pub new_label: Option<String>,
    pub open: bool,
    pub duplicate: bool,
    /// Rows of the « Télécharger » submenu.
    pub download_items: Vec<WsMenuItem>,
    pub rename: bool,
    pub details: bool,
    pub details_label: Option<String>,
    pub trash: bool,
    pub undo: bool,
    pub can_undo: bool,
    pub redo: bool,
    pub can_redo: bool,
    pub cut: bool,
    pub copy: bool,
    pub paste: bool,
    pub find_replace: bool,
}

/// `buildWorkspaceMenus`: Fichier / Édition / Affichage / (extra) / Aide, with
/// the web's labels and shortcuts. The rows' ids are `ws.new`, `ws.open`,
/// `ws.duplicate`, `ws.rename`, `ws.trash`, `ws.details`, `ws.undo`,
/// `ws.redo`, `ws.cut`, `ws.copy`, `ws.paste`, `ws.find_replace`,
/// `ws.fullscreen`, `ws.help`, `ws.forum`, `ws.about`.
pub fn build_workspace_menus(a: &WorkspaceMenuActions, extra: Vec<WsMenu>) -> Vec<WsMenu> {
    let act = |id: &str, label: &str, on: bool| WsMenuItem::action(id, label).enabled(on);
    let file = vec![
        act("ws.new", a.new_label.as_deref().unwrap_or("Nouveau"), a.new).shortcut("Ctrl+N"),
        act("ws.open", "Ouvrir", a.open).shortcut("Ctrl+O"),
        act("ws.duplicate", "Créer une copie", a.duplicate),
        WsMenuItem::Separator,
        if a.download_items.is_empty() {
            act("ws.download", "Télécharger", false)
        } else {
            WsMenuItem::Submenu { label: "Télécharger".into(), items: a.download_items.clone() }
        },
        act("ws.rename", "Renommer", a.rename),
        act("ws.trash", "Mettre à la corbeille", a.trash),
        WsMenuItem::Separator,
        act("ws.details", a.details_label.as_deref().unwrap_or("Détails"), a.details),
    ];
    let edit = vec![
        act("ws.undo", "Annuler", a.undo && a.can_undo).shortcut("Ctrl+Z"),
        act("ws.redo", "Rétablir", a.redo && a.can_redo).shortcut("Ctrl+Shift+Z"),
        WsMenuItem::Separator,
        act("ws.cut", "Couper", a.cut).shortcut("Ctrl+X"),
        act("ws.copy", "Copier", a.copy).shortcut("Ctrl+C"),
        act("ws.paste", "Coller", a.paste).shortcut("Ctrl+V"),
        WsMenuItem::Separator,
        act("ws.find_replace", "Rechercher et remplacer", a.find_replace).shortcut("Ctrl+H"),
    ];
    let view = vec![WsMenuItem::action("ws.fullscreen", "Plein écran").shortcut("F11")];
    let help = vec![
        WsMenuItem::action("ws.help", "Aide en ligne"),
        WsMenuItem::action("ws.forum", "Forum"),
        WsMenuItem::Separator,
        WsMenuItem::action("ws.about", "À propos"),
    ];
    let mut out = vec![WsMenu::new("Fichier", file), WsMenu::new("Édition", edit), WsMenu::new("Affichage", view)];
    out.extend(extra);
    out.push(WsMenu::new("Aide", help));
    out
}

// ═════════════════════════════════════════════════════════════════════════════
// MenuBar
// ═════════════════════════════════════════════════════════════════════════════

/// Which of the web's two bars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuBarStyle {
    /// `WorkspaceMenuBar`: 28 DIP, `#ffffff` / `#1c1c1e`, 12 DIP labels.
    Workspace,
    /// PaintSharp's `MenuBar`: 24 DIP, in the theme's `header`.
    Compact,
}

/// A menu bar: a row of labels, each opening its menu.
pub struct MenuBar {
    pub menus: Vec<WsMenu>,
    pub style: MenuBarStyle,
    open: Option<usize>,
    menu: Option<Menu>,
    prev_down: bool,
    items: Vec<Rect>,
}

impl MenuBar {
    pub fn new(menus: Vec<WsMenu>, style: MenuBarStyle) -> Self {
        Self { menus, style, open: None, menu: None, prev_down: false, items: Vec::new() }
    }

    /// The bar's height.
    pub fn height(&self) -> f32 {
        match self.style {
            MenuBarStyle::Workspace => 28.0,
            MenuBarStyle::Compact => 24.0,
        }
    }

    /// Whether a menu is open.
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    fn item_rects(&self, c: &dyn Canvas, bounds: Rect) -> Vec<Rect> {
        let f = ui_format(c, 12.0, 400);
        let (pad, x0, h) = match self.style {
            MenuBarStyle::Workspace => (8.0, bounds.left + 4.0, 20.0),
            MenuBarStyle::Compact => (10.0, bounds.left + 4.0, 24.0),
        };
        let cy = (bounds.top + bounds.bottom - 1.0) / 2.0;
        let mut x = x0;
        self.menus
            .iter()
            .map(|m| {
                let w = c.measure(&m.label, &f).ceil() + pad * 2.0;
                let r = Rect::new(x, cy - h / 2.0, x + w, cy + h / 2.0);
                x += w;
                r
            })
            .collect()
    }

    fn open_at(&mut self, i: usize) {
        self.open = Some(i);
        self.menu = self.menus.get(i).map(WsMenu::menu);
    }

    fn panel(&mut self, c: &dyn Canvas, area: Rect) -> Option<Rect> {
        let i = self.open?;
        let anchor = *self.items.get(i)?;
        let menu = self.menu.as_mut()?;
        menu.viewport = Some(area);
        let want = menu.measure(c);
        let w = want.width.max(256.0);
        let e = lists::VIEWPORT_EDGE;
        let x = anchor.left.min(area.right - e - w).max(area.left + e);
        let y = anchor.bottom + 2.0;
        Some(Rect::new(x, y, x + w, y + want.height))
    }

    /// One frame of the bar in `bounds` (the full-width strip): returns the
    /// id of a chosen row.
    pub fn frame(&mut self, c: &dyn Canvas, bounds: Rect, f: &Frame, theme: &WorkspaceTheme) -> Option<String> {
        let (mx, my) = f.mouse;
        let pressed = f.mouse_down && !self.prev_down;
        let released = !f.mouse_down && self.prev_down;
        self.prev_down = f.mouse_down;
        self.items = self.item_rects(c, bounds);
        let area = f.screen_area();
        if f.dismiss {
            self.open = None;
            self.menu = None;
        }
        let over_label = if f.pointer_outside() { None } else { self.items.iter().position(|r| r.contains(mx, my)) };
        let panel = self.panel(c, area);
        let sub = match (self.menu.as_ref(), panel) {
            (Some(m), Some(p)) => m.open_submenu.and_then(|i| m.submenu_rect_in(c, p, i)),
            _ => None,
        };
        let mut chosen: Option<String> = None;
        // Labels: click toggles, hover switches while one is open.
        if let Some(i) = over_label {
            if pressed {
                if self.open == Some(i) {
                    self.open = None;
                    self.menu = None;
                } else {
                    self.open_at(i);
                }
            } else if self.open.is_some() && self.open != Some(i) {
                self.open_at(i);
            }
            host::set_cursor(Cursor::Arrow);
        } else if let (Some(menu), Some(panel)) = (self.menu.as_mut(), panel) {
            if let Some(out) = menu_pointer(c, menu, panel, sub, mx, my, released) {
                if let MenuOutcome::Chosen { index, sub } = out {
                    chosen = self.open.and_then(|o| self.menus.get(o)).and_then(|m| m.id_at(index, sub));
                }
            } else if pressed {
                self.open = None;
                self.menu = None;
            }
        }
        // Keys while a menu is open: the menu's own, plus Left / Right across the bar.
        if self.open.is_some() {
            for k in take_menu_keys() {
                let n = self.menus.len().max(1);
                let in_sub = self.menu.as_ref().is_some_and(|m| m.submenu_hot.is_some());
                let on_parent = self.menu.as_ref().is_some_and(|m| m.hot_index.and_then(|h| m.submenu(h)).is_some());
                match k {
                    MenuKey::Left if !in_sub => {
                        let i = self.open.unwrap_or(0);
                        self.open_at((i + n - 1) % n);
                        if let Some(m) = self.menu.as_mut() {
                            m.hot_index = m.next_actionable(None, true);
                        }
                    }
                    // Right on a row with a submenu opens it (below); elsewhere it
                    // moves to the next menu of the bar.
                    MenuKey::Right if !in_sub && !on_parent => {
                        let i = self.open.unwrap_or(0);
                        self.open_at((i + 1) % n);
                        if let Some(m) = self.menu.as_mut() {
                            m.hot_index = m.next_actionable(None, true);
                        }
                    }
                    k => {
                        let out = self.menu.as_mut().map(|m| m.navigate(k)).unwrap_or(MenuOutcome::Ignored);
                        match out {
                            MenuOutcome::Chosen { index, sub } => {
                                chosen = self.open.and_then(|o| self.menus.get(o)).and_then(|m| m.id_at(index, sub));
                            }
                            MenuOutcome::Close => {
                                self.open = None;
                                self.menu = None;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
        if chosen.is_some() {
            self.open = None;
            self.menu = None;
        }

        // ── Paint ────────────────────────────────────────────────────────
        let (bg, rule, ink, hover, hover_soft) = match self.style {
            MenuBarStyle::Workspace if theme.dark => (hex(0x1c1c1e), rgba(255, 255, 255, 0.08), hex(0xcccccc), rgba(255, 255, 255, 0.12), rgba(255, 255, 255, 0.08)),
            MenuBarStyle::Workspace => (hex(0xffffff), theme.border, theme.text, rgba(0, 0, 0, 0.08), rgba(0, 0, 0, 0.06)),
            MenuBarStyle::Compact => (theme.header, theme.border, theme.text, theme.active, theme.active),
        };
        fill(c, bounds, bg);
        fill(c, Rect::new(bounds.left, bounds.bottom - 1.0, bounds.right, bounds.bottom), rule);
        let fmt = ui_format(c, 12.0, 400);
        let radius = if self.style == MenuBarStyle::Workspace { 4.0 } else { 2.0 };
        for (i, (m, r)) in self.menus.iter().zip(self.items.iter()).enumerate() {
            let open = self.open == Some(i);
            if open {
                c.fill_rounded(r, radius, &hover);
            } else if over_label == Some(i) {
                c.fill_rounded(r, radius, &hover_soft);
            }
            c.text(&m.label, r, &fmt, &ink, true);
        }
        if let Some(panel) = self.panel(c, area) {
            if let Some(menu) = self.menu.as_ref() {
                show_menu(c, menu, panel, area);
            }
        }
        chosen
    }
}

/// The pointer over an open menu (and its cascaded submenu): hover moves the
/// highlight and opens / closes the cascade; a release on a leaf chooses it.
/// `None` when the pointer is on neither.
pub(crate) fn menu_pointer(c: &dyn Canvas, menu: &mut Menu, panel: Rect, sub: Option<Rect>, x: f32, y: f32, released: bool) -> Option<MenuOutcome> {
    let _ = c;
    if let (Some(sr), Some(parent)) = (sub.filter(|r| r.contains(x, y)), menu.open_submenu) {
        let child = menu.submenu(parent)?;
        let j = child.item_at(sr, x, y).filter(|&j| child.is_actionable(j));
        if let Some(j) = j {
            menu.submenu_hot = Some(j);
            menu.hot_index = Some(parent);
            if released {
                return Some(MenuOutcome::Chosen { index: parent, sub: Some(j) });
            }
        }
        return Some(MenuOutcome::Ignored);
    }
    if !panel.contains(x, y) {
        return None;
    }
    if let Some(i) = menu.item_at(panel, x, y).filter(|&i| menu.is_actionable(i)) {
        menu.hot_index = Some(i);
        menu.submenu_hot = None;
        menu.open_submenu = menu.submenu(i).is_some().then_some(i);
        if released && menu.submenu(i).is_none() {
            return Some(MenuOutcome::Chosen { index: i, sub: None });
        }
    }
    Some(MenuOutcome::Ignored)
}

fn take_menu_keys() -> Vec<MenuKey> {
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

fn show_menu(c: &dyn Canvas, menu: &Menu, panel: Rect, area: Rect) {
    let pb = menu.paint_bounds(c, panel);
    let mut snap = menu.clone();
    snap.viewport = Some(Rect::new(area.left - pb.left, area.top - pb.top, area.right - pb.left, area.bottom - pb.top));
    let local = Rect::new(panel.left - pb.left, panel.top - pb.top, panel.right - pb.left, panel.bottom - pb.top);
    host::popup(pb, move |cv| snap.paint(cv, local, WidgetState::REST));
}

// ═════════════════════════════════════════════════════════════════════════════
// WorkspaceShell
// ═════════════════════════════════════════════════════════════════════════════

/// What the user did, and where the host paints its parts.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceRun {
    /// The back arrow was clicked.
    pub back: bool,
    /// The trash button was clicked (the web confirms first: the host shows
    /// its `ConfirmDialog`).
    pub delete: bool,
    /// The search button was clicked.
    pub search: bool,
    /// A menu row was chosen (its id).
    pub menu: Option<String>,
    /// The topbar's right-hand area, `topbar_actions_width` wide.
    pub topbar_actions: Rect,
    pub options_bar: Option<Rect>,
    pub tool_rail: Option<Rect>,
    /// The body (typically a dock).
    pub body: Rect,
    pub bottom_bar: Option<Rect>,
    pub status_bar: Option<Rect>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShellHit {
    Back,
    Delete,
    Search,
}

/// `WorkspaceShell`: the chrome of an advanced application.
pub struct WorkspaceShell {
    pub theme: WorkspaceTheme,
    pub title: String,
    /// A Lucide geometry shown before the title.
    pub title_icon: Option<&'static str>,
    /// The editor's name (`"Layer"`), in the accent.
    pub subtitle: Option<String>,
    /// Dimensions, page count…
    pub doc_info: Option<String>,
    pub show_back: bool,
    pub show_delete: bool,
    pub show_search: bool,
    /// Width reserved on the right of the topbar for the host's buttons.
    pub topbar_actions_width: f32,
    pub topbar_height: f32,
    /// `0` = no options bar.
    pub options_bar_height: f32,
    /// `0` = no tool rail.
    pub tool_rail_width: f32,
    /// `0` = no bottom bar.
    pub bottom_bar_height: f32,
    /// `0` = no status bar.
    pub status_height: f32,
    /// Texts of the status bar, laid out left to right (`gap-4`).
    pub status: Vec<String>,
    /// The menu bar, if any.
    pub menu_bar: Option<MenuBar>,
    pressed: Option<ShellHit>,
    prev_down: bool,
}

impl Default for WorkspaceShell {
    fn default() -> Self {
        Self::new(WorkspaceTheme::dark())
    }
}

impl WorkspaceShell {
    /// The web's defaults: 40 DIP topbar, 30 DIP options bar (when used), a 44
    /// DIP tool rail (when used), 22 DIP status bar.
    pub fn new(theme: WorkspaceTheme) -> Self {
        Self {
            theme,
            title: String::new(),
            title_icon: None,
            subtitle: None,
            doc_info: None,
            show_back: false,
            show_delete: false,
            show_search: false,
            topbar_actions_width: 0.0,
            topbar_height: 40.0,
            options_bar_height: 0.0,
            tool_rail_width: 0.0,
            bottom_bar_height: 0.0,
            status_height: 0.0,
            status: Vec::new(),
            menu_bar: None,
            pressed: None,
            prev_down: false,
        }
    }

    /// Builder: the standard menus (`menuActions` + `extraMenus`), in the
    /// workspace bar.
    pub fn with_menus(mut self, menus: Vec<WsMenu>, style: MenuBarStyle) -> Self {
        self.menu_bar = Some(MenuBar::new(menus, style));
        self
    }

    /// One frame of the shell in `bounds`.
    pub fn frame(&mut self, c: &dyn Canvas, bounds: Rect, f: &Frame) -> WorkspaceRun {
        let th = self.theme;
        let mut run = WorkspaceRun::default();
        let (mx, my) = f.mouse;
        let pressed = f.mouse_down && !self.prev_down;
        let released = !f.mouse_down && self.prev_down;
        self.prev_down = f.mouse_down;
        fill(c, bounds, th.bg);

        // ── Topbar ──────────────────────────────────────────────────────
        let topbar_bg = th.topbar_bg.unwrap_or(hex(0x111111));
        let tb = Rect::new(bounds.left, bounds.top, bounds.right, bounds.top + self.topbar_height);
        fill(c, tb, topbar_bg);
        if th.topbar_text.is_none() {
            fill(c, Rect::new(tb.left, tb.bottom - 1.0, tb.right, tb.bottom), th.border);
        }
        let tb_dark = th.dark || th.topbar_text.is_some();
        let tb_color = th.topbar_text.unwrap_or(th.text);
        let tb_dim = th.topbar_text.unwrap_or(th.text_dim);
        let hover_bg = if tb_dark { rgba(255, 255, 255, 0.10) } else { c.theme().surface_2 };
        let cy = (tb.top + tb.bottom) / 2.0;
        let btn = |x: f32| Rect::new(x, cy - 14.0, x + 28.0, cy + 14.0);
        let mut x = tb.left + 8.0;
        let mut hits: Vec<(ShellHit, Rect)> = Vec::new();
        if self.show_back {
            hits.push((ShellHit::Back, btn(x)));
            x += 28.0 + 8.0;
        }
        if let Some(icon) = self.title_icon {
            c.vector_icon(icon, &Rect::new(x, cy - 10.0, x + 20.0, cy + 10.0), 18.0, &tb_color);
            x += 20.0 + 8.0;
        }
        let actions_w = self.topbar_actions_width.max(0.0);
        let right_limit = tb.right - 8.0 - actions_w - if self.show_search { 36.0 } else { 0.0 };
        if !self.title.is_empty() {
            // `text-sm` / `text-xs` (`WorkspaceShell.tsx`): the body and meta roles.
            let tf = ui_format(c, crate::metrics::text::BODY, 500);
            let w = c.measure(&self.title, &tf).ceil().min(320.0);
            c.text_ellipsis(&self.title, &Rect::new(x, tb.top, (x + w).min(right_limit), tb.bottom), &tf, &tb_color);
            x += w + 8.0;
            let sf = ui_format(c, crate::metrics::text::META, 400);
            if let Some(s) = &self.subtitle {
                let w = c.measure(s, &sf).ceil();
                c.text(s, &Rect::new(x, tb.top, x + w, tb.bottom), &sf, &th.topbar_text.unwrap_or(th.accent), false);
                x += w + 8.0;
            }
            if let Some(s) = &self.doc_info {
                let w = c.measure(s, &sf).ceil();
                c.text(s, &Rect::new(x, tb.top, x + w, tb.bottom), &sf, &tb_dim, false);
                x += w + 8.0;
            }
        }
        if self.show_delete {
            hits.push((ShellHit::Delete, btn(x)));
        }
        let mut rx = tb.right - 8.0 - actions_w;
        run.topbar_actions = Rect::new(rx, tb.top, tb.right - 8.0, tb.bottom);
        if self.show_search {
            rx -= 28.0 + 8.0;
            hits.push((ShellHit::Search, btn(rx)));
        }
        let hot = if f.pointer_outside() { None } else { hits.iter().find(|(_, r)| r.contains(mx, my)).map(|(h, _)| *h) };
        if pressed {
            self.pressed = hot;
        }
        if released {
            if let (Some(p), Some(h)) = (self.pressed.take(), hot) {
                if p == h {
                    match h {
                        ShellHit::Back => run.back = true,
                        ShellHit::Delete => run.delete = true,
                        ShellHit::Search => run.search = true,
                    }
                }
            }
        }
        for (h, r) in &hits {
            if hot == Some(*h) {
                c.fill_rounded(r, 4.0, &hover_bg);
                host::set_cursor(Cursor::Hand);
            }
            let (icon, size) = match h {
                ShellHit::Back => ("ArrowLeft", 16.0),
                ShellHit::Delete => ("Trash2", 15.0),
                ShellHit::Search => ("Search", 16.0),
            };
            let ink = if th.topbar_text.is_some() { tb_color } else if th.dark { th.text_dim } else { c.theme().text_secondary };
            c.vector_icon(icon, r, size, &ink);
        }
        let mut y = tb.bottom;

        // ── Menu bar ────────────────────────────────────────────────────
        if let Some(mb) = self.menu_bar.as_mut() {
            let r = Rect::new(bounds.left, y, bounds.right, y + mb.height());
            run.menu = mb.frame(c, r, f, &th);
            y = r.bottom;
        }

        // ── Options bar ─────────────────────────────────────────────────
        if self.options_bar_height > 0.0 {
            let r = Rect::new(bounds.left, y, bounds.right, y + self.options_bar_height);
            fill(c, r, th.header);
            fill(c, Rect::new(r.left, r.bottom - 1.0, r.right, r.bottom), th.border);
            run.options_bar = Some(Rect::new(r.left + 12.0, r.top, r.right - 12.0, r.bottom - 1.0));
            y = r.bottom;
        }

        // ── Status + bottom bar (from the bottom up) ────────────────────
        let mut yb = bounds.bottom;
        if self.status_height > 0.0 {
            let r = Rect::new(bounds.left, yb - self.status_height, bounds.right, yb);
            fill(c, r, th.status_bg.unwrap_or(hex(0x111111)));
            fill(c, Rect::new(r.left, r.top, r.right, r.top + 1.0), th.border);
            let sf = ui_format(c, 10.0, 400);
            let mut sx = r.left + 16.0;
            for s in &self.status {
                let w = c.measure(s, &sf).ceil();
                c.text(s, &Rect::new(sx, r.top + 1.0, sx + w, r.bottom), &sf, &th.text_dim, false);
                sx += w + 16.0;
            }
            run.status_bar = Some(Rect::new(r.left + 16.0, r.top + 1.0, r.right - 16.0, r.bottom));
            yb = r.top;
        }
        if self.bottom_bar_height > 0.0 {
            let r = Rect::new(bounds.left, yb - self.bottom_bar_height, bounds.right, yb);
            run.bottom_bar = Some(r);
            yb = r.top;
        }

        // ── Tool rail + body ────────────────────────────────────────────
        let mut bx = bounds.left;
        if self.tool_rail_width > 0.0 {
            let r = Rect::new(bx, y, bx + self.tool_rail_width, yb);
            fill(c, r, th.toolbar);
            fill(c, Rect::new(r.right - 1.0, r.top, r.right, r.bottom), th.border);
            run.tool_rail = Some(Rect::new(r.left, r.top + 8.0, r.right - 1.0, r.bottom - 8.0));
            bx = r.right;
        }
        run.body = Rect::new(bx, y, bounds.right, yb.max(y));
        let _ = Modifiers::NONE;
        run
    }

    /// The same palette as a dock theme (`theme={C}` on the web).
    pub fn dock_theme(&self) -> crate::dock::DockTheme {
        let t = &self.theme;
        crate::dock::DockTheme {
            panel: t.panel,
            header: t.header,
            border: t.border,
            text: t.text,
            text_dim: t.text_dim,
            accent: t.accent,
            ground: None,
            radius: None,
            gap: None,
            tab_active_bg: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_menus_follow_the_web_order_and_grey_missing_actions() {
        let menus = build_workspace_menus(&WorkspaceMenuActions { undo: true, can_undo: true, ..Default::default() }, vec![WsMenu::new("Insertion", vec![])]);
        let labels: Vec<&str> = menus.iter().map(|m| m.label.as_str()).collect();
        assert_eq!(labels, ["Fichier", "Édition", "Affichage", "Insertion", "Aide"]);
        let enabled = |m: &WsMenu, id: &str| {
            m.items.iter().find_map(|i| match i {
                WsMenuItem::Action { id: x, enabled, .. } if x == id => Some(*enabled),
                _ => None,
            })
        };
        assert_eq!(enabled(&menus[1], "ws.undo"), Some(true));
        assert_eq!(enabled(&menus[1], "ws.redo"), Some(false));
        assert_eq!(enabled(&menus[0], "ws.new"), Some(false));
    }

    #[test]
    fn menu_ids_resolve_through_submenus() {
        let a = WorkspaceMenuActions { download_items: vec![WsMenuItem::action("dl.pdf", "PDF")], ..Default::default() };
        let menus = build_workspace_menus(&a, vec![]);
        let file = &menus[0];
        let dl = file.items.iter().position(|i| matches!(i, WsMenuItem::Submenu { .. })).expect("download submenu");
        assert_eq!(file.id_at(dl, Some(0)).as_deref(), Some("dl.pdf"));
        assert_eq!(file.id_at(0, None).as_deref(), Some("ws.new"));
        assert_eq!(file.id_at(3, None), None, "a separator has no id");
    }
}
