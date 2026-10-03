//! The menu family (`vskubuno/docs/MENUS.md`): what `<MenuBar>`, `<ContextMenu>`, `<MenuItem>`,
//! `<MenuSeparator>`, `<MenuHeader>`, `<DropDownButton>` and `<SplitButton>` share beyond the menu model
//! of [`crate::window`] (which reads every menu of a view into a [`MenuSpec`] and opens it):
//!
//! - the view's `<Command>`s as menus read them ([`CommandSpec`]): an item's `Command` gives it its
//!   text, icon, shortcut, enabled and checked state, and runs its `OnExecute`;
//! - the keyboard accelerators ([`Accelerator`]): every `ShortcutKeys` (and every `Command` shortcut no
//!   ribbon handles) runs its item while its menu is closed, before the focused control reads the key
//!   (WinForms' `ProcessCmdKey`);
//! - the menu bar ([`MenuBarNode`], WinForms `MenuStrip`): its labels, the state the runtime keeps for
//!   it ([`BarInfo`], the open and keyboard-hot item), Alt and F10;
//! - the drop-down and split buttons ([`DropDownButtonNode`], WinForms `ToolStripDropDownButton` and
//!   `ToolStripSplitButton`);
//! - the design-time rendering of a menu: open on the surface while one of its elements is selected,
//!   every item selectable, with the « Type Here » slots the designer types new items into
//!   ([`TypeSlot`]).

use std::cell::RefCell;
use std::collections::HashMap;

use kubuno_desktop_controls::host::{self, vk, Modifiers};
use kubuno_desktop_ui::lists::{Menu, MenuEntry};
use kubuno_desktop_ui::{Canvas, FocusId, Rect, Size, Widget, WidgetState};
use kubuno_desktop_views_syntax::shortcut::{Key, Shortcut};

use crate::ast::{AstNode, Element};
use crate::binding::{BindingSpec, PropSource, ViewModel};
use crate::design::{parent_id_of, LayoutEntry};
use crate::node::{PaintCx, ViewEventKind, ViewNode};
use crate::registry::LayoutKind;
use crate::window::{MenuAnchor, MenuItemSpec, MenuRequest, MenuSpec};

// ── References and mnemonics ───────────────────────────────────────────────────────────────

/// The element a reference property names: `menu1`, `{x:Ref menu1}` or `{x:Reference menu1}`
/// (WPF's spelling). Empty for anything else in braces (a binding is not a reference).
pub fn reference_name(raw: &str) -> String {
    let t = raw.trim();
    let Some(inner) = t.strip_prefix('{').and_then(|r| r.strip_suffix('}')) else { return t.to_string() };
    let inner = inner.trim();
    for prefix in ["x:Reference", "x:Ref"] {
        if let Some(rest) = inner.strip_prefix(prefix) {
            let rest = rest.trim();
            return rest.strip_prefix("Name=").unwrap_or(rest).trim().to_string();
        }
    }
    String::new()
}

/// The mnemonic letter of a menu text (`&File` → `f`), lower case.
pub fn mnemonic_of(text: &str) -> Option<char> {
    kubuno_desktop_views_syntax::shortcut::mnemonic_key(text)
}

/// The name of the menu of a menu bar's top-level item `item_id` (its sub-items): never a valid
/// `x:Name`, so it never meets a `<ContextMenu>`'s.
pub fn bar_menu_name(item_id: &str) -> String {
    format!("#bar:{item_id}")
}

/// The name of the menu a drop-down or split button `button_id` holds as its own items.
pub fn drop_down_menu_name(button_id: &str) -> String {
    format!("#dd:{button_id}")
}

// ── Commands ───────────────────────────────────────────────────────────────────────────────

/// A `<Command>` of the view as menus read it (`RIBBON.md` §4).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CommandSpec {
    /// Its stable id (the sender of its `OnExecute`).
    pub id: String,
    pub name: String,
    pub label: String,
    /// The Lucide glyph of its `SmallIcon`.
    pub icon: String,
    pub shortcut: String,
    /// Its `ScreenTipText` (else its `ScreenTipTitle`), the tooltip of a menu item running it.
    pub tooltip: String,
    pub enabled: bool,
    pub checked: bool,
    /// `IsCheckable`: running it switches `Checked`.
    pub checkable: bool,
    pub on_execute: Option<String>,
    /// Its bound `Label`, `Enabled` and `Checked`.
    pub bindings: Vec<(&'static str, BindingSpec)>,
}

impl CommandSpec {
    /// The command with its bindings read from `vm`; `checks` holds the state of a checkable command
    /// whose `Checked` is not bound (keyed by its id).
    pub fn resolved(&self, vm: &dyn ViewModel, checks: &HashMap<String, bool>) -> Self {
        let mut c = self.clone();
        for (property, spec) in &self.bindings {
            let Some(value) = crate::resources::get(vm, spec) else { continue };
            match (*property, value) {
                ("Label", v) => c.label = crate::binding::Row::new().with("v", v).text("v"),
                ("Enabled", crate::binding::Value::Bool(b)) => c.enabled = b,
                ("Checked", crate::binding::Value::Bool(b)) => c.checked = b,
                _ => {}
            }
        }
        if let Some(state) = checks.get(&self.id) {
            if !self.bindings.iter().any(|(p, _)| *p == "Checked") {
                c.checked = *state;
            }
        }
        c
    }

    /// The binding of its `Checked`, when it has one.
    pub fn checked_binding(&self) -> Option<&BindingSpec> {
        self.bindings.iter().find(|(p, _)| *p == "Checked").map(|(_, b)| b)
    }
}

/// Every `<Command x:Name>` of the view.
pub fn read_commands(root: &Element) -> Vec<CommandSpec> {
    let attr = |e: &Element, name: &str| e.attribute(name).and_then(|a| a.value());
    root.syntax()
        .descendants()
        .filter_map(Element::cast)
        .filter(|e| e.name().as_deref() == Some("Command"))
        .filter_map(|e| {
            let name = attr(&e, "x:Name").filter(|n| !n.is_empty())?;
            let mut bindings = Vec::new();
            let mut literal = |prop: &'static str| -> Option<String> {
                let raw = attr(&e, prop)?;
                if crate::binding::is_binding_expr(&raw) {
                    if let Some(spec) = crate::binding::parse_binding(&raw) {
                        bindings.push((prop, spec));
                    }
                    return None;
                }
                Some(raw)
            };
            let label = literal("Label").unwrap_or_default();
            let enabled = literal("Enabled").is_none_or(|v| v.trim() != "false");
            let checked = literal("Checked").is_some_and(|v| v.trim() == "true");
            let tip = attr(&e, "ScreenTipText").filter(|t| !t.is_empty()).or_else(|| attr(&e, "ScreenTipTitle")).unwrap_or_default();
            Some(CommandSpec {
                id: e.stable_id(),
                name,
                label,
                icon: crate::icon::attribute(&e, "SmallIcon").map(str::to_string).unwrap_or_default(),
                shortcut: attr(&e, "Shortcut").unwrap_or_default(),
                tooltip: tip,
                enabled,
                checked,
                checkable: attr(&e, "IsCheckable").is_some_and(|v| v.trim() == "true"),
                on_execute: attr(&e, "OnExecute").filter(|h| !h.is_empty()),
                bindings,
            })
        })
        .collect()
}

// ── Accelerators ───────────────────────────────────────────────────────────────────────────

/// A keyboard shortcut of the view and what it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Accelerator {
    pub shortcut: Shortcut,
    pub target: AcceleratorTarget,
}

/// What an [`Accelerator`] runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcceleratorTarget {
    /// The item of id `item` of the menu `menu` (its `OnClick`, its check mark, its command).
    Item { menu: String, item: String },
    /// A `<Command>` no menu item runs, in a view without a ribbon (a ribbon runs its commands' shortcuts itself).
    Command(String),
}

/// The accelerators of a view: every item of `menus` with a shortcut (its own `ShortcutKeys`, else
/// its command's), and the shortcuts of the `commands` no item runs when the view has no ribbon. The
/// first one wins when two share a shortcut (the language server warns about it).
pub fn accelerators(menus: &[MenuSpec], commands: &[CommandSpec], has_ribbon: bool) -> Vec<Accelerator> {
    fn walk(menu: &MenuSpec, items: &[MenuItemSpec], commands: &[CommandSpec], out: &mut Vec<Accelerator>, used: &mut Vec<String>) {
        for item in items {
            let command = item.command.as_deref().and_then(|n| commands.iter().find(|c| c.name == n));
            if let Some(c) = command {
                used.push(c.name.clone());
            }
            let text = if item.shortcut.trim().is_empty() { command.map(|c| c.shortcut.as_str()).unwrap_or("") } else { item.shortcut.as_str() };
            if let Ok(shortcut) = kubuno_desktop_views_syntax::shortcut::parse(text) {
                if !out.iter().any(|a| a.shortcut == shortcut) {
                    out.push(Accelerator { shortcut, target: AcceleratorTarget::Item { menu: menu.name.clone(), item: item.id.clone() } });
                }
            }
            walk(menu, &item.children, commands, out, used);
        }
    }
    let mut out = Vec::new();
    let mut used = Vec::new();
    for menu in menus {
        walk(menu, &menu.items, commands, &mut out, &mut used);
    }
    if !has_ribbon {
        for c in commands.iter().filter(|c| !used.contains(&c.name)) {
            if let Ok(shortcut) = kubuno_desktop_views_syntax::shortcut::parse(&c.shortcut) {
                if !out.iter().any(|a| a.shortcut == shortcut) {
                    out.push(Accelerator { shortcut, target: AcceleratorTarget::Command(c.name.clone()) });
                }
            }
        }
    }
    out
}

/// The Windows virtual-key code of `key`.
pub fn vk_of(key: Key) -> u16 {
    match key {
        Key::Letter(c) => vk::letter(c),
        Key::Digit(d) => vk::digit(d),
        Key::Function(f) => 0x70 + u16::from(f) - 1,
        Key::Delete => vk::DELETE,
        Key::Insert => vk::INSERT,
        Key::Home => vk::HOME,
        Key::End => vk::END,
        Key::PageUp => vk::PAGE_UP,
        Key::PageDown => vk::PAGE_DOWN,
        Key::Up => vk::UP,
        Key::Down => vk::DOWN,
        Key::Left => vk::LEFT,
        Key::Right => vk::RIGHT,
        Key::Enter => vk::ENTER,
        Key::Escape => vk::ESCAPE,
        Key::Space => vk::SPACE,
        Key::Tab => vk::TAB,
        Key::Backspace => vk::BACK,
        Key::Plus => 0xBB,
        Key::Minus => 0xBD,
        Key::Comma => 0xBC,
        Key::Period => 0xBE,
        Key::Slash => 0xBF,
        Key::Semicolon => 0xBA,
        Key::Apps => vk::APPS,
    }
}

/// Whether the key `code` pressed with `mods` is `shortcut` (the numeric keypad's + and − count as
/// Plus and Minus).
pub fn matches(shortcut: &Shortcut, code: u16, mods: Modifiers) -> bool {
    let key = vk_of(shortcut.key);
    let same_key = code == key || (shortcut.key == Key::Plus && code == 0x6B) || (shortcut.key == Key::Minus && code == 0x6D);
    same_key && mods.ctrl == shortcut.ctrl && mods.shift == shortcut.shift && mods.alt == shortcut.alt
}

// ── The menu bars' runtime state ───────────────────────────────────────────────────────────

/// A menu bar painted this frame, as the runtime reads it at the next one.
#[derive(Debug, Clone, PartialEq)]
pub struct BarInfo {
    /// The bar's element id.
    pub id: String,
    pub items: Vec<BarItem>,
    /// Its `OnMenuActivate` / `OnMenuDeactivate` handlers.
    pub on_activate: Option<String>,
    pub on_deactivate: Option<String>,
}

/// One top-level item of a [`BarInfo`].
#[derive(Debug, Clone, PartialEq)]
pub struct BarItem {
    /// Its menu's name ([`bar_menu_name`]).
    pub menu: String,
    /// Its label's box (client DIP).
    pub rect: Rect,
    pub enabled: bool,
    /// Its mnemonic letter.
    pub key: Option<char>,
}

#[derive(Default)]
struct BarState {
    /// The bar whose menu is open, and the index of its item.
    open: Option<(String, usize)>,
    /// The bar in keyboard mode (Alt or F10 pressed alone), and its hot item.
    focus: Option<(String, usize)>,
    /// A press on the open item's label closed its menu: the label must not reopen it.
    suppress: Option<(String, usize)>,
}

thread_local! {
    static BAR: RefCell<BarState> = RefCell::new(BarState::default());
    static RUN_ITEMS: RefCell<Vec<(String, String)>> = const { RefCell::new(Vec::new()) };
    static TYPE_SLOTS: RefCell<Vec<TypeSlot>> = const { RefCell::new(Vec::new()) };
    static DESIGN_VIEW: std::cell::Cell<Option<Rect>> = const { std::cell::Cell::new(None) };
}

/// The bar menu now open (set by the runtime).
pub(crate) fn set_open_bar(open: Option<(String, usize)>) {
    BAR.with(|b| b.borrow_mut().open = open);
}

/// The item of bar `bar` whose menu is open.
pub fn open_bar_item(bar: &str) -> Option<usize> {
    BAR.with(|b| b.borrow().open.as_ref().filter(|(id, _)| id == bar).map(|(_, i)| *i))
}

/// The bar in keyboard mode (set by the runtime).
pub(crate) fn set_bar_focus(focus: Option<(String, usize)>) {
    BAR.with(|b| b.borrow_mut().focus = focus);
}

/// The keyboard-hot item of bar `bar`.
pub fn bar_focus(bar: &str) -> Option<usize> {
    BAR.with(|b| b.borrow().focus.as_ref().filter(|(id, _)| id == bar).map(|(_, i)| *i))
}

/// A press on the label of bar `bar`'s item `index` closed its menu this frame.
pub(crate) fn suppress_bar_press(bar: &str, index: usize) {
    BAR.with(|b| b.borrow_mut().suppress = Some((bar.to_string(), index)));
}

fn take_suppressed(bar: &str, index: usize) -> bool {
    BAR.with(|b| {
        let mut b = b.borrow_mut();
        let hit = b.suppress.as_ref().is_some_and(|(id, i)| id == bar && *i == index);
        if hit {
            b.suppress = None;
        }
        hit
    })
}

/// Asks the runtime to run the item `item` of the menu `menu` at the next frame (a split button's
/// `DefaultItem`).
pub(crate) fn request_run_item(menu: &str, item: &str) {
    RUN_ITEMS.with(|r| r.borrow_mut().push((menu.to_string(), item.to_string())));
    host::request_repaint_after(0);
}

/// The items asked to run (see [`request_run_item`]), taken.
pub(crate) fn take_run_items() -> Vec<(String, String)> {
    RUN_ITEMS.with(|r| std::mem::take(&mut *r.borrow_mut()))
}

/// The view's box in the designer (set by the runtime before a design frame): where a
/// `<ContextMenu>` being designed shows.
pub(crate) fn set_design_view(bounds: Option<Rect>) {
    DESIGN_VIEW.with(|d| d.set(bounds));
}

fn design_view() -> Option<Rect> {
    DESIGN_VIEW.with(std::cell::Cell::get)
}

// ── Design time: « Type Here » ─────────────────────────────────────────────────────────────

/// A « Type Here » slot the designer draws at the end of a menu level or a menu bar: typing in it
/// inserts a `<MenuItem>` as child `index` of the element `parent_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeSlot {
    pub rect: Rect,
    pub parent_id: String,
    /// The index of the new element among the parent's element children.
    pub index: usize,
    /// A slot of a menu bar (items go left to right; their own items below them).
    pub horizontal: bool,
}

/// Declares a slot drawn this frame.
pub fn push_type_slot(slot: TypeSlot) {
    TYPE_SLOTS.with(|s| s.borrow_mut().push(slot));
}

/// The slots of the frame that just painted, taken (the design surface types into them).
pub fn take_type_slots() -> Vec<TypeSlot> {
    let slots = TYPE_SLOTS.with(|s| std::mem::take(&mut *s.borrow_mut()));
    LAST_SLOTS.with(|l| *l.borrow_mut() = slots.clone());
    slots
}

/// A row of a menu shown on the design surface (or a label of a menu bar): where a dragged row may go.
#[derive(Debug, Clone, PartialEq)]
pub struct MenuRow {
    pub id: String,
    pub parent: String,
    pub rect: Rect,
    /// A label of a menu bar (its rows go left to right).
    pub horizontal: bool,
}

thread_local! {
    static ROWS: RefCell<Vec<MenuRow>> = const { RefCell::new(Vec::new()) };
    static LAST_ROWS: RefCell<Vec<MenuRow>> = const { RefCell::new(Vec::new()) };
    static LAST_SLOTS: RefCell<Vec<TypeSlot>> = const { RefCell::new(Vec::new()) };
    static DRAG_HOVER: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn push_row(row: MenuRow) {
    ROWS.with(|r| r.borrow_mut().push(row));
}

/// Ends a design frame's rows: they become what the next drag reads ([`drop_target`]).
pub fn finish_design_rows() {
    let rows = ROWS.with(|r| std::mem::take(&mut *r.borrow_mut()));
    LAST_ROWS.with(|l| *l.borrow_mut() = rows);
}

/// Whether `id` is a menu row of the last design frame.
pub fn is_menu_row(id: &str) -> bool {
    LAST_ROWS.with(|l| l.borrow().iter().any(|r| r.id == id))
}

/// While a menu row is dragged, the row under the pointer: its sub-menu opens so the row can be dropped
/// into it (set by the design surface each frame, `None` otherwise).
pub fn set_drag_hover(id: Option<String>) {
    DRAG_HOVER.with(|d| *d.borrow_mut() = id);
}

/// What a dragged menu row is over at `(x, y)` (last design frame): a row of a menu, else the owner
/// of the « Type Here » slot under it — what [`set_drag_hover`] takes.
pub fn hover_at(x: f32, y: f32) -> Option<String> {
    let row = LAST_ROWS.with(|l| l.borrow().iter().rev().find(|r| r.rect.contains(x, y)).map(|r| r.id.clone()));
    row.or_else(|| LAST_SLOTS.with(|l| l.borrow().iter().find(|s| s.rect.contains(x, y)).map(|s| s.parent_id.clone())))
}

/// An `x:Name` for a menu item typed as `text` (`&Ouvrir…` → `ouvrir_item`): its letters folded to
/// ASCII, unique among `taken`.
pub fn item_name(text: &str, taken: &std::collections::HashSet<String>) -> String {
    let shown = crate::common::mnemonic(text).0;
    let mut slug = String::new();
    for c in shown.chars().flat_map(char::to_lowercase) {
        let folded = match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' => "a",
            'é' | 'è' | 'ê' | 'ë' => "e",
            'î' | 'ï' | 'í' => "i",
            'ô' | 'ö' | 'ó' | 'õ' => "o",
            'ù' | 'û' | 'ü' | 'ú' => "u",
            'ç' => "c",
            'ñ' => "n",
            'œ' => "oe",
            'æ' => "ae",
            c if c.is_ascii_alphanumeric() => {
                slug.push(c);
                continue;
            }
            _ => "_",
        };
        slug.push_str(folded);
    }
    let mut slug = slug.split('_').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("_");
    if slug.is_empty() || slug.starts_with(|c: char| c.is_ascii_digit()) {
        slug.insert_str(0, "menu_");
    }
    crate::edit::unique_name(&format!("{slug}_item"), taken)
}

fn drag_hover() -> Option<String> {
    DRAG_HOVER.with(|d| d.borrow().clone())
}

/// Where the dragged menu row `dragged` goes when dropped at `(x, y)`: before or after the row under
/// the pointer, in that row's level, or at the end of the level whose « Type Here » slot is under it —
/// the parent element, the index among its element children, and the insertion marker. `None` over
/// nothing of a menu, or inside the dragged row's own sub-menu.
pub fn drop_target(dragged: &str, x: f32, y: f32) -> Option<(String, usize, Rect)> {
    let inside_dragged = |parent: &str| parent == dragged || parent.starts_with(&format!("{dragged}."));
    let rows = LAST_ROWS.with(|l| l.borrow().clone());
    if let Some(row) = rows.iter().rev().find(|r| r.rect.contains(x, y) && r.id != dragged) {
        if inside_dragged(&row.parent) {
            return None;
        }
        let ordinal: usize = row.id.rsplit('.').next().and_then(|s| s.parse().ok())?;
        let after = if row.horizontal { x > (row.rect.left + row.rect.right) / 2.0 } else { y > (row.rect.top + row.rect.bottom) / 2.0 };
        // Within its own level, the index counts the list without the dragged row (`move_child`).
        let same = parent_id_of(dragged).as_deref() == Some(row.parent.as_str());
        let dragged_ordinal: usize = dragged.rsplit('.').next().and_then(|s| s.parse().ok()).unwrap_or(usize::MAX);
        let mut index = ordinal + usize::from(after);
        if same && dragged_ordinal < index {
            index -= 1;
        }
        let r = row.rect;
        let marker = if row.horizontal {
            let at = if after { r.right } else { r.left };
            Rect::new(at - 1.5, r.top, at + 1.5, r.bottom)
        } else {
            let at = if after { r.bottom } else { r.top };
            Rect::new(r.left + 6.0, at - 1.5, r.right - 6.0, at + 1.5)
        };
        return Some((row.parent.clone(), index, marker));
    }
    let slots = LAST_SLOTS.with(|l| l.borrow().clone());
    let slot = slots.iter().find(|s| s.rect.contains(x, y))?;
    if inside_dragged(&slot.parent_id) {
        return None;
    }
    let r = slot.rect;
    Some((slot.parent_id.clone(), slot.index, Rect::new(r.left + 6.0, r.top - 1.5, r.right - 6.0, r.top + 1.5)))
}

/// The text of an empty slot.
pub fn type_here_text() -> String {
    crate::messages::tr("Type Here", "Tapez ici")
}

/// Whether the designer's selection is `id` or inside it.
fn selected_under(selection: &[String], id: &str) -> bool {
    !id.is_empty() && selection.iter().any(|s| s == id || s.starts_with(&format!("{id}.")))
}

/// What an item shows in the designer: its text, else its bound text as written, else its command's
/// label.
fn design_text(item: &MenuItemSpec, commands: &[CommandSpec]) -> String {
    if !item.text.is_empty() {
        return item.text.clone();
    }
    if let Some((_, spec)) = item.bindings.iter().find(|(p, _)| *p == "Text") {
        return format!("{{{}}}", spec.path);
    }
    item.command.as_deref().and_then(|n| commands.iter().find(|c| c.name == n)).map(|c| c.label.clone()).unwrap_or_default()
}

/// The strip row of an item in the designer (its mnemonic and its sub-menu chevron included).
fn design_row(item: &MenuItemSpec, commands: &[CommandSpec]) -> kubuno_desktop_controls::toolstrip::StripItem {
    if item.is_separator() {
        return kubuno_desktop_ui::lists::separator();
    }
    let (text, _) = crate::common::mnemonic(&design_text(item, commands));
    if item.header {
        return kubuno_desktop_ui::lists::section(text);
    }
    let command = item.command.as_deref().and_then(|n| commands.iter().find(|c| c.name == n));
    let mut e = MenuEntry::new(text).checked(item.checked).enabled(item.enabled);
    let shortcut = if item.shortcut_text().is_empty() && !item.hide_shortcut { command.map(|c| c.shortcut.clone()).unwrap_or_default() } else { item.shortcut_text().to_string() };
    if !shortcut.is_empty() {
        e = e.shortcut_text(shortcut);
    }
    let icon = if item.icon.is_empty() { command.map(|c| c.icon.clone()).unwrap_or_default() } else { item.icon.clone() };
    if !icon.is_empty() {
        e = e.icon(icon);
    }
    if item.danger {
        e = e.danger();
    }
    if !item.children.is_empty() || item.items_source.is_some() {
        e = e.submenu(vec![MenuEntry::new("…").build()]);
    }
    e.build()
}

/// One level of a menu laid out on the design surface.
struct DesignLevel {
    menu: Menu,
    panel: Rect,
}

/// Lays out, records and defers the paint of a menu shown open on the design surface: the level of
/// `items` (children of the element `owner_id`, which has `owner_children` element children) at
/// `at`, and the sub-level of each selected item to its right, each ending with a « Type Here »
/// slot. Every row is a selectable element of the layout map (`Flow`: dragging reorders it).
#[allow(clippy::too_many_arguments)]
pub(crate) fn design_menu(cx: &mut PaintCx<'_>, at: (f32, f32), owner_id: &str, owner_children: usize, items: &[MenuItemSpec], commands: &[CommandSpec], caption: Option<String>, min_width: f32) {
    let selection = crate::virtual_regions::design_selection();
    let c: &dyn Canvas = cx.canvas;
    let mut levels: Vec<DesignLevel> = Vec::new();
    let mut entries: Vec<LayoutEntry> = Vec::new();
    let mut slots: Vec<TypeSlot> = Vec::new();
    let mut add_glyphs: Vec<(Rect, String)> = Vec::new();
    let mut level_items: Vec<MenuItemSpec> = items.to_vec();
    let mut level_owner = owner_id.to_string();
    let mut level_count = owner_children;
    let anchor = at;
    let mut parent_row: Option<Rect> = None;
    for _depth in 0..16 {
        let mut rows: Vec<_> = level_items.iter().map(|i| design_row(i, commands)).collect();
        // The « Type Here » row: a dimmed command, outlined below.
        rows.push(MenuEntry::new(type_here_text()).enabled(false).build());
        let mut menu = Menu::with_items(rows);
        menu.mnemonics = level_items.iter().map(|i| crate::common::mnemonic(&design_text(i, commands)).1.map(|(_, at)| at)).collect();
        let want = menu.measure(c);
        let width = want.width.max(min_width);
        let panel = match parent_row {
            None => Rect::new(anchor.0, anchor.1, anchor.0 + width, anchor.1 + want.height),
            Some(row) => Rect::new(row.right - 2.0, row.top - 4.0, row.right - 2.0 + width, row.top - 4.0 + want.height),
        };
        let mut open: Option<(usize, Rect)> = None;
        for (i, item) in level_items.iter().enumerate() {
            let Some(row) = menu.item_rect(panel, i) else { continue };
            entries.push(LayoutEntry { id: item.id.clone(), parent_id: parent_id_of(&item.id), bounds: row, layout: LayoutKind::Flow, container: false, locked: false, clip: None });
            push_row(MenuRow { id: item.id.clone(), parent: level_owner.clone(), rect: row, horizontal: false });
            if open.is_none() && item.is_command() && !item.header && selected_under(&selection, &item.id) {
                open = Some((i, row));
            }
        }
        // A row dragged over another row: that row's sub-menu opens, so the drag can go into it.
        if let Some(hover) = drag_hover() {
            if let Some((i, item)) = level_items.iter().enumerate().find(|(_, it)| selected_under(std::slice::from_ref(&hover), &it.id) && it.is_command()) {
                if let Some(row) = menu.item_rect(panel, i) {
                    let _ = item;
                    open = Some((i, row));
                }
            }
        }
        if let Some(slot) = menu.item_rect(panel, level_items.len()) {
            slots.push(TypeSlot { rect: slot, parent_id: level_owner.clone(), index: level_count, horizontal: false });
            add_glyphs.push((Rect::new(slot.right - 22.0, slot.top + 6.0, slot.right - 8.0, slot.bottom - 6.0), level_owner.clone()));
        }
        levels.push(DesignLevel { menu, panel });
        let Some((i, row)) = open else { break };
        let item = &level_items[i];
        level_owner = item.id.clone();
        level_count = item.children.len() + usize::from(item.item_template.is_some());
        level_items = item.children.clone();
        parent_row = Some(row);
    }
    let caption_chip = caption.zip(levels.first().map(|l| l.panel));
    for slot in &slots {
        push_type_slot(slot.clone());
    }
    for (rect, owner) in &add_glyphs {
        crate::virtual_regions::push_design_glyph(crate::virtual_regions::DesignGlyph { rect: *rect, element_id: owner.clone(), menu: "add" });
    }
    let paint_slots: Vec<Rect> = slots.iter().map(|s| s.rect).collect();
    let glyphs: Vec<Rect> = add_glyphs.iter().map(|(r, _)| *r).collect();
    crate::virtual_regions::defer_late(
        entries,
        Box::new(move |c| {
            if let Some((text, panel)) = caption_chip {
                paint_caption_chip(c, &text, panel);
            }
            for level in &levels {
                level.menu.paint(c, level.panel, WidgetState::REST);
            }
            for slot in paint_slots {
                paint_slot_outline(c, slot);
            }
            for g in glyphs {
                paint_slot_arrow(c, g);
            }
        }),
    );
}

/// The dashed outline of a « Type Here » slot.
fn paint_slot_outline(c: &dyn kubuno_desktop_controls::ControlCanvas, slot: Rect) {
    let r = Rect::new(slot.left + 6.0, slot.top + 2.0, slot.right - 6.0, slot.bottom - 2.0);
    let ink = c.theme().text_tertiary;
    for dash in crate::design::dashed_outline(r, 3.0, 2.0, 1.0) {
        c.fill_rect(&dash, &ink);
    }
}

/// The small « ▾ » of a slot: what else can be added there (a separator, a header…).
fn paint_slot_arrow(c: &dyn kubuno_desktop_controls::ControlCanvas, r: Rect) {
    let t = c.theme();
    c.stroke_rect(&r, &t.text_tertiary);
    c.vector_icon("ChevronDown", &r, 10.0, &t.text_secondary);
}

/// The tab above a context menu being designed, with its name (WinForms' designer shows one).
fn paint_caption_chip(c: &dyn kubuno_desktop_controls::ControlCanvas, text: &str, panel: Rect) {
    let t = c.theme();
    let f = &c.formats().caption;
    let w = c.measure(text, f).ceil() + 16.0;
    let r = Rect::new(panel.left, panel.top - 20.0, panel.left + w, panel.top - 2.0);
    c.fill_rounded(&r, 3.0, &t.accent);
    c.text(text, &r, f, &t.accent_foreground, true);
}

// ── <ContextMenu> ──────────────────────────────────────────────────────────────────────────

/// `<ContextMenu>`: nothing at run time (the window opens it, `crate::window`); in the designer, shown
/// open at the top of the view while it or one of its items is selected (WinForms' designer does the
/// same from its component tray).
pub struct ContextMenuNode {
    pub id: String,
    pub name: String,
    pub items: Vec<MenuItemSpec>,
    pub children: usize,
    pub commands: Vec<CommandSpec>,
}

impl ContextMenuNode {
    pub fn build(element: &Element) -> Self {
        let root = element.syntax().ancestors().filter_map(Element::cast).last();
        Self {
            id: element.stable_id(),
            name: element.attribute("x:Name").and_then(|a| a.value()).unwrap_or_else(|| "ContextMenu".into()),
            items: crate::window::read_items(element),
            children: element.children().count(),
            commands: root.map(|r| read_commands(&r)).unwrap_or_default(),
        }
    }
}

impl ViewNode for ContextMenuNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn ViewModel) -> Size {
        Size { width: 0.0, height: 0.0 }
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        if cx.design.is_none() || !selected_under(&crate::virtual_regions::design_selection(), &self.id) {
            return;
        }
        let view = design_view().unwrap_or(bounds);
        let at = (view.left + 16.0, view.top + 30.0);
        design_menu(cx, at, &self.id, self.children, &self.items, &self.commands, Some(self.name.clone()), 180.0);
        smart_tag_at(&self.id, Rect::new(at.0 - 18.0, at.1 - 20.0, at.0 - 4.0, at.1 - 6.0));
    }
}

/// A smart tag (its tasks: Insert Standard Items, Edit Items…) for the element `id`, drawn at `rect`.
fn smart_tag_at(id: &str, rect: Rect) {
    if crate::virtual_regions::design_selection().first().map(String::as_str) != Some(id) {
        return;
    }
    crate::virtual_regions::push_design_glyph(crate::virtual_regions::DesignGlyph { rect, element_id: id.to_string(), menu: "tasks" });
    crate::virtual_regions::defer_late(Vec::new(), Box::new(move |c| crate::virtual_regions::paint_smart_tag(c, rect)));
}

// ── <MenuBar> ──────────────────────────────────────────────────────────────────────────────

/// `<MenuBar>` (WinForms `MenuStrip`, the web's `WorkspaceMenuBar`): a row of labels, each opening its
/// menu below it. A click opens a menu; while one is open, the pointer moving over another label
/// opens that one; Alt or F10 alone puts the bar in keyboard mode (Left/Right, Down or Enter opens,
/// a letter opens the menu it is the mnemonic of); Alt + a mnemonic opens its menu directly.
pub struct MenuBarNode {
    pub id: String,
    pub items: Vec<MenuItemSpec>,
    /// The number of element children (the index of a new item).
    pub children: usize,
    pub compact: bool,
    pub focus_id: Option<FocusId>,
    pub on_activate: Option<String>,
    pub on_deactivate: Option<String>,
    pub commands: Vec<CommandSpec>,
    was_down: bool,
}

impl MenuBarNode {
    pub fn build(element: &Element, compact: bool, focus_id: Option<FocusId>) -> Self {
        let root = element.syntax().ancestors().filter_map(Element::cast).last();
        let attr = |n: &str| element.attribute(n).and_then(|a| a.value()).filter(|h| !h.is_empty());
        Self {
            id: element.stable_id(),
            items: element.children().filter(|c| c.name().as_deref() == Some("MenuItem")).map(|c| crate::window::read_item(&c)).collect(),
            children: element.children().count(),
            compact,
            focus_id,
            on_activate: attr("OnMenuActivate"),
            on_deactivate: attr("OnMenuDeactivate"),
            commands: root.map(|r| read_commands(&r)).unwrap_or_default(),
            was_down: false,
        }
    }

    fn height(&self) -> f32 {
        if self.compact { 24.0 } else { 28.0 }
    }

    /// The label of each top-level item (bindings and `{Res}` read from `vm`), its `&` kept.
    fn labels(&self, vm: &dyn ViewModel, design: bool) -> Vec<String> {
        self.items
            .iter()
            .map(|i| {
                if design {
                    return design_text(i, &self.commands);
                }
                match i.bindings.iter().find(|(p, _)| *p == "Text").and_then(|(_, spec)| crate::resources::get(vm, spec)) {
                    Some(v) => crate::binding::Row::new().with("v", v).text("v"),
                    None => design_text(i, &self.commands),
                }
            })
            .collect()
    }

    fn rects(&self, c: &dyn Canvas, bounds: Rect, labels: &[String]) -> Vec<Rect> {
        let f = &c.formats().caption;
        let (pad, h) = if self.compact { (10.0, 24.0) } else { (8.0, 20.0) };
        let cy = (bounds.top + bounds.bottom - 1.0) / 2.0;
        let mut x = bounds.left + 4.0;
        labels
            .iter()
            .map(|l| {
                let w = c.measure(&crate::common::mnemonic(l).0, f).ceil() + pad * 2.0;
                let r = Rect::new(x, cy - h / 2.0, x + w, cy + h / 2.0);
                x += w;
                r
            })
            .collect()
    }
}

impl ViewNode for MenuBarNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let labels = self.labels(vm, false);
        let rects = self.rects(c, Rect::new(0.0, 0.0, 10_000.0, self.height()), &labels);
        Size::new(rects.last().map_or(40.0, |r| r.right + 4.0), self.height())
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let design = cx.design.is_some();
        let labels = self.labels(&*cx.vm, design);
        let rects = self.rects(cx.canvas, bounds, &labels);
        let frame = cx.frame;
        let (mx, my) = frame.mouse;
        let pressed = frame.mouse_down && !self.was_down;
        self.was_down = frame.mouse_down;
        let open = open_bar_item(&self.id);
        let keyboard = bar_focus(&self.id);
        let hover = if frame.pointer_outside() || design { None } else { rects.iter().position(|r| r.contains(mx, my)) };
        let enabled: Vec<bool> = self.items.iter().map(|i| i.enabled).collect();
        if !design {
            // A click on a label opens its menu (a click on the open one closes it: the runtime).
            if let Some(i) = hover.filter(|_| pressed) {
                if !take_suppressed(&self.id, i) && open != Some(i) && enabled[i] {
                    let mut request = MenuRequest::new(bar_menu_name(&self.items[i].id), MenuAnchor::Below(crate::common::to_client(rects[i])));
                    request.bar = Some((self.id.clone(), i));
                    request.owner = self.items[i].id.clone();
                    crate::window::request_menu(request);
                }
            }
            if let Some(services) = cx.services.as_deref_mut() {
                // Alt + a mnemonic, or an accessibility client's Invoke: its menu opens from the keyboard.
                for (i, item) in self.items.iter().enumerate() {
                    let element = format!("{}#{i}", self.id);
                    if let Some(key) = mnemonic_of(&labels[i]) {
                        services.mnemonics.push(crate::common::Mnemonic { key, element: element.clone(), action: crate::common::MnemonicAction::Activate });
                    }
                    if services.take_activation(&element) && enabled[i] {
                        let mut request = MenuRequest::new(bar_menu_name(&item.id), MenuAnchor::Below(crate::common::to_client(rects[i])));
                        request.bar = Some((self.id.clone(), i));
                        request.keyboard = true;
                        request.owner = item.id.clone();
                        crate::window::request_menu(request);
                    }
                }
                services.menu_bars.push(BarInfo {
                    id: self.id.clone(),
                    items: self
                        .items
                        .iter()
                        .enumerate()
                        .map(|(i, it)| BarItem { menu: bar_menu_name(&it.id), rect: crate::common::to_client(rects[i]), enabled: enabled[i], key: mnemonic_of(&labels[i]) })
                        .collect(),
                    on_activate: self.on_activate.clone(),
                    on_deactivate: self.on_deactivate.clone(),
                });
                // Assistive technology: each label is a menu item of the bar, expanded while its menu is open.
                let parent = crate::common::access_id(&self.id);
                for (i, label) in labels.iter().enumerate() {
                    let element = format!("{}#{i}", self.id);
                    let id = crate::common::access_id(&element);
                    let client = crate::common::to_client(rects[i]);
                    services.access_ids.push((id, element, None));
                    let (name, key) = crate::common::mnemonic(label);
                    services.access.push(kubuno_desktop_controls::host::access::AccessNode {
                        id,
                        parent: Some(parent),
                        role: kubuno_desktop_controls::host::access::AccessRole::MenuItem,
                        name,
                        bounds: (client.left, client.top, client.right, client.bottom),
                        disabled: !enabled[i],
                        clickable: enabled[i],
                        read_only: true,
                        access_key: key.map(|(k, _)| format!("Alt+{}", k.to_ascii_uppercase())),
                        expanded: Some(open == Some(i)),
                        ..Default::default()
                    });
                }
            }
        }
        // ── Paint (the web's WorkspaceMenuBar / PaintSharp's compact bar) ──
        let c = cx.canvas;
        let t = c.theme();
        let dark = t.mode == kubuno_drive_desktop_app_controls::ThemeMode::Dark;
        let rgba = |r: u8, g: u8, b: u8, a: f32| kubuno_desktop_controls::styled::D2D1_COLOR_F { r: f32::from(r) / 255.0, g: f32::from(g) / 255.0, b: f32::from(b) / 255.0, a };
        let (bg, rule, ink, open_fill, hover_fill) = match (self.compact, dark) {
            (false, true) => (rgba(0x1c, 0x1c, 0x1e, 1.0), rgba(255, 255, 255, 0.08), rgba(0xcc, 0xcc, 0xcc, 1.0), rgba(255, 255, 255, 0.12), rgba(255, 255, 255, 0.08)),
            (false, false) => (rgba(255, 255, 255, 1.0), t.divider, t.text_primary, rgba(0, 0, 0, 0.08), rgba(0, 0, 0, 0.06)),
            (true, _) => (t.layer_background, t.divider, t.text_primary, t.row_hover, t.row_hover),
        };
        c.fill_rounded(&bounds, 0.0, &bg);
        c.fill_rounded(&Rect::new(bounds.left, bounds.bottom - 1.0, bounds.right, bounds.bottom), 0.0, &rule);
        let f = &c.formats().caption;
        let radius = if self.compact { 2.0 } else { 4.0 };
        let selection = if design { crate::virtual_regions::design_selection() } else { Vec::new() };
        let cues = design || frame.mods.alt || keyboard.is_some();
        for (i, r) in rects.iter().enumerate() {
            let lit = open == Some(i) || (design && selected_under(&selection, &self.items[i].id) && !selection.iter().any(|s| s == &self.items[i].id));
            if lit {
                c.fill_rounded(r, radius, &open_fill);
            } else if hover == Some(i) || keyboard == Some(i) {
                c.fill_rounded(r, radius, &hover_fill);
            }
            let (text, mnemonic) = crate::common::mnemonic(&labels[i]);
            let ink = if enabled[i] { ink } else { kubuno_desktop_controls::styled::D2D1_COLOR_F { a: ink.a * 0.45, ..ink } };
            c.text(&text, r, f, &ink, true);
            if let (true, Some((_, at))) = (cues, mnemonic) {
                kubuno_desktop_ui::mnemonic::underline(c, &text, at, r, f, &ink, windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_CENTER);
            }
        }
        if !design {
            return;
        }
        // ── Design: every label is an element; the selected one's menu shows below it; « Type Here » ends the bar.
        if let Some(map) = cx.design.as_mut() {
            for (i, item) in self.items.iter().enumerate() {
                map.push(LayoutEntry { id: item.id.clone(), parent_id: parent_id_of(&item.id), bounds: rects[i], layout: LayoutKind::Flow, container: false, locked: false, clip: None });
                push_row(MenuRow { id: item.id.clone(), parent: self.id.clone(), rect: rects[i], horizontal: true });
            }
        }
        if selected_under(&selection, &self.id) {
            let x = rects.last().map_or(bounds.left + 4.0, |r| r.right + 2.0);
            let w = c.measure(&type_here_text(), f).ceil() + 16.0;
            let slot = Rect::new(x, rects.first().map_or(bounds.top + 4.0, |r| r.top), x + w, rects.first().map_or(bounds.bottom - 4.0, |r| r.bottom));
            push_type_slot(TypeSlot { rect: slot, parent_id: self.id.clone(), index: self.children, horizontal: true });
            let text = type_here_text();
            let dim = t.text_tertiary;
            crate::virtual_regions::defer_late(
                Vec::new(),
                Box::new(move |c| {
                    for dash in crate::design::dashed_outline(slot, 3.0, 2.0, 1.0) {
                        c.fill_rect(&dash, &dim);
                    }
                    c.text(&text, &slot, &c.formats().caption, &dim, true);
                }),
            );
            smart_tag_at(&self.id, Rect::new(bounds.right - 18.0, bounds.top + 4.0, bounds.right - 4.0, bounds.top + 18.0));
        }
        let hover = drag_hover();
        let opened = |id: &str| match &hover {
            Some(h) if self.items.iter().any(|it| selected_under(std::slice::from_ref(h), &it.id)) => selected_under(std::slice::from_ref(h), id),
            _ => selected_under(&selection, id),
        };
        if let Some((i, item)) = self.items.iter().enumerate().find(|(_, it)| opened(&it.id)) {
            let count = item.children.len() + usize::from(item.item_template.is_some());
            let children = item.children.clone();
            let id = item.id.clone();
            let commands = self.commands.clone();
            design_menu(cx, (rects[i].left, rects[i].bottom + 2.0), &id, count, &children, &commands, None, 200.0);
        }
    }
}

// ── <DropDownButton> and <SplitButton> ─────────────────────────────────────────────────────

/// `<DropDownButton>` (WinForms `ToolStripDropDownButton`): a button whose click opens its menu below
/// it — its own items, else the `<ContextMenu>` its `DropDownMenu` names. `<SplitButton>`
/// (`ToolStripSplitButton`): a button whose main part raises `OnClick` (or runs its `DefaultItem`)
/// and whose arrow opens the menu. Space, Enter, Down and Alt+Down open the menu from the keyboard
/// (Space and Enter click a split button's main part).
pub struct DropDownButtonNode {
    pub id: String,
    pub split: bool,
    pub text: PropSource<String>,
    pub variant: PropSource<String>,
    pub size: PropSource<String>,
    pub icon: PropSource<String>,
    pub base: crate::common::ButtonBaseProps,
    pub focus_id: Option<FocusId>,
    /// The menu it opens: its own items' ([`drop_down_menu_name`]), else its `DropDownMenu`.
    pub menu: Option<String>,
    /// It holds its items (else a `DropDownMenu` reference, whose opening it announces itself).
    pub own_items: bool,
    pub items: Vec<MenuItemSpec>,
    pub children: usize,
    pub show_arrow: bool,
    pub on_click: Option<String>,
    pub on_opening: Option<String>,
    /// `DefaultItem`: the `x:Name` of the item a split button's main part runs without `OnClick`.
    pub default_item: Option<String>,
    pub commands: Vec<CommandSpec>,
    pressed: Option<bool>,
}

/// The width of a split button's arrow part, in DIP.
const ARROW_PART: f32 = 26.0;

impl DropDownButtonNode {
    #[allow(clippy::too_many_arguments)]
    pub fn build(element: &Element, split: bool, text: PropSource<String>, variant: PropSource<String>, size: PropSource<String>, icon: PropSource<String>, base: crate::common::ButtonBaseProps, focus_id: Option<FocusId>, drop_down: Option<String>) -> Self {
        let root = element.syntax().ancestors().filter_map(Element::cast).last();
        let attr = |n: &str| element.attribute(n).and_then(|a| a.value()).filter(|h| !h.is_empty());
        let id = element.stable_id();
        let items = crate::window::read_items(element);
        let own_items = !items.is_empty();
        Self {
            menu: if own_items { Some(drop_down_menu_name(&id)) } else { drop_down.map(|m| reference_name(&m)).filter(|m| !m.is_empty()) },
            own_items,
            children: element.children().count(),
            items,
            id,
            split,
            text,
            variant,
            size,
            icon,
            base,
            focus_id,
            show_arrow: attr("ShowDropDownArrow").is_none_or(|v| v.trim() != "false"),
            on_click: attr("OnClick"),
            on_opening: attr("OnDropDownOpening"),
            default_item: attr("DefaultItem"),
            commands: root.map(|r| read_commands(&r)).unwrap_or_default(),
            pressed: None,
        }
    }

    fn face(&self, vm: &dyn ViewModel, with_arrow: bool) -> kubuno_desktop_ui::buttons::Button {
        let text = self.text.resolve(vm);
        let shown = if self.base.use_mnemonic { crate::common::mnemonic(&text).0 } else { text };
        let mut b = kubuno_desktop_ui::buttons::Button::new(&shown).variant(crate::node::parse_variant(&self.variant.resolve(vm))).size(crate::node::parse_button_size(&self.size.resolve(vm)));
        if let Some(name) = crate::icon::resolve(&self.icon.resolve(vm)) {
            b = b.icon(name);
        }
        let _ = with_arrow;
        b
    }

    /// The main part and the arrow part of a split button.
    fn parts(&self, bounds: Rect) -> (Rect, Rect) {
        let split = (bounds.right - ARROW_PART).max(bounds.left);
        (Rect::new(bounds.left, bounds.top, split - 1.0, bounds.bottom), Rect::new(split, bounds.top, bounds.right, bounds.bottom))
    }

    fn open(&self, cx: &mut PaintCx<'_>, bounds: Rect, keyboard: bool) {
        let Some(menu) = self.menu.clone() else { return };
        if !self.own_items {
            // A referenced `<ContextMenu>` raises its own OnOpening; the button announces its drop-down too.
            cx.fire("OnDropDownOpening", self.focus_id, self.on_opening.as_deref(), ViewEventKind::Other { name: "DropDownOpening", args: std::rc::Rc::new(crate::events::CancelEventArgs::default()) }, &mut crate::events::CancelEventArgs::default());
        }
        let mut request = MenuRequest::new(menu, MenuAnchor::Below(crate::common::to_client(bounds)));
        request.keyboard = keyboard;
        request.owner = self.id.clone();
        crate::window::request_menu(request);
    }
}

impl ViewNode for DropDownButtonNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let s = self.face(vm, false).measure(c);
        if self.split {
            Size::new(s.width + ARROW_PART + 1.0, s.height)
        } else if self.show_arrow {
            Size::new(s.width + CHEVRON_GAP + CHEVRON, s.height)
        } else {
            s
        }
    }

    fn intrinsic_width(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Option<f32> {
        Some(self.measure(c, vm).width)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let design = cx.design.is_some();
        let frame = cx.frame;
        let (mx, my) = frame.mouse;
        let (main, arrow) = if self.split { self.parts(bounds) } else { (bounds, bounds) };
        let inside = |r: Rect| !frame.pointer_outside() && r.contains(mx, my);
        let focus = self.focus_id.map(|id| cx.focus.register(id, bounds)).unwrap_or_default();
        let open_now = !design && self.menu.as_deref().is_some_and(crate::window::is_menu_open);
        // Press on a part, release on the same part: its click.
        let mut clicked: Option<bool> = None; // Some(true): the arrow.
        if !design {
            let on_arrow = self.split && inside(arrow);
            let on_main = inside(main) && !on_arrow;
            if frame.mouse_down && self.pressed.is_none() && (on_main || on_arrow) {
                self.pressed = Some(on_arrow || !self.split);
                // A drop-down opens on the press, like a menu.
                if self.pressed == Some(true) && !open_now {
                    clicked = Some(true);
                }
            }
            if !frame.mouse_down {
                if self.pressed == Some(false) && on_main {
                    clicked = Some(false);
                }
                self.pressed = None;
            }
            if cx.activate {
                clicked = Some(!self.split);
            }
            if focus.focused {
                if host::take_key(vk::DOWN, Modifiers::ALT) + host::take_key(vk::DOWN, Modifiers::NONE) + host::take_key(vk::F4, Modifiers::NONE) > 0 {
                    self.open(cx, bounds, true);
                } else if host::take_key(vk::SPACE, Modifiers::NONE) + host::take_key(vk::ENTER, Modifiers::NONE) > 0 {
                    if self.split {
                        clicked = Some(false);
                    } else {
                        self.open(cx, bounds, true);
                    }
                }
            }
        }
        match clicked {
            Some(true) => self.open(cx, bounds, false),
            Some(false) => {
                if self.on_click.is_some() || self.default_item.is_none() {
                    cx.fire("OnClick", self.focus_id, self.on_click.as_deref(), ViewEventKind::Clicked, &mut crate::node::click_args(frame, bounds));
                } else if let (Some(name), Some(menu)) = (self.default_item.as_deref(), self.menu.as_deref()) {
                    fn find<'a>(items: &'a [MenuItemSpec], name: &str) -> Option<&'a MenuItemSpec> {
                        items.iter().find_map(|i| if i.name.as_deref() == Some(name) { Some(i) } else { find(&i.children, name) })
                    }
                    if let Some(item) = find(&self.items, name) {
                        request_run_item(menu, &item.id);
                    }
                }
            }
            None => {}
        }
        // ── Paint ──
        let c = cx.canvas;
        let state = |r: Rect, part_pressed: bool| focus.apply(crate::common::rest().hot(!design && inside(r)).pressed(part_pressed || (open_now && !self.split)));
        let raw = self.text.resolve(&*cx.vm);
        let (_, underline) = cx.mnemonic_text(&raw, self.base.use_mnemonic, crate::common::MnemonicAction::Activate);
        if self.split {
            let mut face = self.face(&*cx.vm, false);
            face.mnemonic = underline;
            face.paint(c, main, state(main, self.pressed == Some(false)));
            let chevron = kubuno_desktop_ui::buttons::Button::new("").variant(crate::node::parse_variant(&self.variant.resolve(&*cx.vm))).size(crate::node::parse_button_size(&self.size.resolve(&*cx.vm))).icon("ChevronDown");
            chevron.paint(c, arrow, state(arrow, self.pressed == Some(true) || open_now));
        } else {
            let st = state(bounds, self.pressed.is_some());
            let mut face = self.face(&*cx.vm, false);
            face.mnemonic = underline;
            if self.show_arrow {
                // The button's ground (fill, border, focus ring), then its icon, label and chevron laid out here.
                kubuno_desktop_ui::buttons::Button::new("").variant(crate::node::parse_variant(&self.variant.resolve(&*cx.vm))).size(crate::node::parse_button_size(&self.size.resolve(&*cx.vm))).paint(c, bounds, st);
                let ink = foreground_of(&self.variant.resolve(&*cx.vm), c);
                let ink = if st.disabled { kubuno_desktop_controls::styled::D2D1_COLOR_F { a: ink.a * 0.5, ..ink } } else { ink };
                paint_drop_down_face(c, bounds, &face, underline, &ink);
            } else {
                face.paint(c, bounds, st);
            }
        }
        if design {
            let selection = crate::virtual_regions::design_selection();
            if (self.own_items || self.menu.is_none()) && selected_under(&selection, &self.id) {
                    let items = self.items.clone();
                    let commands = self.commands.clone();
                    let id = self.id.clone();
                    design_menu(cx, (bounds.left, bounds.bottom + 2.0), &id, self.children, &items, &commands, None, (bounds.right - bounds.left).max(160.0));
            }
            smart_tag_at(&self.id, Rect::new(bounds.right + 2.0, bounds.top, bounds.right + 16.0, bounds.top + 14.0));
        }
    }
}

/// The width of a drop-down button's chevron, and its gap after the label.
const CHEVRON: f32 = 12.0;
const CHEVRON_GAP: f32 = 6.0;

/// A drop-down button's content, centred in `bounds`: its icon, its label (its mnemonic underlined
/// when `underline` says), then the chevron.
fn paint_drop_down_face(c: &dyn kubuno_desktop_controls::ControlCanvas, bounds: Rect, face: &kubuno_desktop_ui::buttons::Button, underline: Option<usize>, ink: &kubuno_desktop_controls::styled::D2D1_COLOR_F) {
    let f = &c.formats().body;
    let label_w = if face.text.is_empty() { 0.0 } else { c.measure(&face.text, f).ceil() };
    let icon = face.icon.map(|_| face.icon_px());
    let gap = face.gap_px();
    let content = icon.map_or(0.0, |s| s + if label_w > 0.0 { gap } else { 0.0 }) + label_w + CHEVRON_GAP + CHEVRON;
    let mut x = ((bounds.left + bounds.right - content) / 2.0).max(bounds.left + 4.0);
    let cy = (bounds.top + bounds.bottom) / 2.0;
    if let (Some(name), Some(size)) = (face.icon, icon) {
        c.vector_icon(name, &Rect::new(x, cy - size / 2.0, x + size, cy + size / 2.0), size, ink);
        x += size + if label_w > 0.0 { gap } else { 0.0 };
    }
    if label_w > 0.0 {
        let band = Rect::new(x - 1.0, cy - 10.0, x + label_w + 1.0, cy + 10.0);
        c.text_ellipsis(&face.text, &band, f, ink);
        if let Some(at) = underline {
            kubuno_desktop_ui::mnemonic::underline(c, &face.text, at, &band, f, ink, windows::Win32::Graphics::DirectWrite::DWRITE_TEXT_ALIGNMENT_LEADING);
        }
        x += label_w;
    }
    x += CHEVRON_GAP;
    c.vector_icon("ChevronDown", &Rect::new(x, cy - CHEVRON / 2.0, x + CHEVRON, cy + CHEVRON / 2.0), CHEVRON, ink);
}

/// The ink of a button's label for `variant` (the chevron takes it).
fn foreground_of(variant: &str, c: &dyn Canvas) -> kubuno_desktop_controls::styled::D2D1_COLOR_F {
    let t = c.theme();
    match variant {
        "Primary" | "" => t.accent_foreground,
        "Danger" => kubuno_desktop_controls::styled::D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 },
        "TextDanger" => t.danger,
        _ => t.text_primary,
    }
}

// ── The language server's checks ───────────────────────────────────────────────────────────

/// The language server's warnings on menus (`MENUS.md` §6), never blocking (the view still runs):
///
/// - a `ShortcutKeys` or a command's `Shortcut` that does not parse (an unknown key, two keys, a
///   letter without Ctrl or Alt…);
/// - two commands of the view with the same shortcut (only the first one would run);
/// - two items of one menu level with the same access key (`&File` and `&Format`);
/// - a `Command` naming no `<Command x:Name>` of the view (outside the ribbon, which checks its own);
/// - a `ContextMenu` or `DropDownMenu` naming no `<ContextMenu x:Name>` of the view.
pub fn warnings(parse: &crate::syntax::Parse, root: &Element) -> Vec<crate::syntax::Diagnostic> {
    use crate::syntax::Diagnostic;
    let mut out = Vec::new();
    let mut warn = |range: Option<rowan::TextRange>, message: String| {
        if let Some(range) = range {
            let lc = parse.line_col(range.start());
            out.push(Diagnostic { range, line: lc.line, column: lc.column, message });
        }
    };
    let attr = |e: &Element, n: &str| e.attribute(n).and_then(|a| a.value()).map(|v| v.trim().to_string()).filter(|v| !v.is_empty() && !crate::binding::is_binding_expr(v));
    let value_range = |e: &Element, n: &str| e.attribute(n).and_then(|a| a.value_range());
    let all: Vec<Element> = root.syntax().descendants().filter_map(Element::cast).collect();
    let named = |kind: &str| -> Vec<String> { all.iter().filter(|e| e.name().as_deref() == Some(kind)).filter_map(|e| attr(e, "x:Name")).collect() };
    let commands = named("Command");
    let menus = named("ContextMenu");

    // Shortcuts: their grammar, then the ones used twice.
    let mut seen: Vec<(Shortcut, u32)> = Vec::new();
    for e in &all {
        let property = match e.name().as_deref() {
            Some("MenuItem") => "ShortcutKeys",
            Some("Command") => "Shortcut",
            _ => continue,
        };
        let Some(text) = attr(e, property) else { continue };
        match kubuno_desktop_views_syntax::shortcut::parse(&text) {
            Err(err) => warn(value_range(e, property), format!("attribute `{property}`: {err}")),
            Ok(shortcut) => {
                let line = value_range(e, property).map(|r| parse.line_col(r.start()).line).unwrap_or(0);
                match seen.iter().find(|(s, _)| *s == shortcut) {
                    Some((_, first)) => warn(value_range(e, property), format!("the shortcut `{shortcut}` is already used on line {first}")),
                    None => seen.push((shortcut, line)),
                }
            }
        }
    }

    for e in &all {
        let Some(name) = e.name() else { continue };
        // Access keys of one menu level (a menu, a sub-menu, a menu bar).
        if matches!(name.as_str(), "ContextMenu" | "MenuItem" | "MenuBar" | "DropDownButton" | "SplitButton") {
            let mut keys: Vec<(char, String)> = Vec::new();
            for item in e.children().filter(|c| c.name().as_deref() == Some("MenuItem")) {
                let Some(text) = attr(&item, "Text") else { continue };
                let Some(key) = mnemonic_of(&text) else { continue };
                let shown = crate::common::mnemonic(&text).0;
                match keys.iter().find(|(k, _)| *k == key) {
                    Some((_, first)) => warn(value_range(&item, "Text"), format!("the access key `{}` is already used by `{first}` in this menu", key.to_ascii_uppercase())),
                    None => keys.push((key, shown)),
                }
            }
        }
        // References: a command (outside the ribbon, which checks its own), a context menu.
        if !name.starts_with("Ribbon") && !name.starts_with("Backstage") {
            if let Some(cmd) = attr(e, "Command").map(|c| reference_name(&c)).filter(|c| !c.is_empty()) {
                if !commands.contains(&cmd) {
                    warn(value_range(e, "Command"), format!("attribute `Command`: no `<Command x:Name=\"{cmd}\">` in this view"));
                }
            }
        }
        for property in ["ContextMenu", "DropDownMenu"] {
            if let Some(menu) = attr(e, property).map(|m| reference_name(&m)).filter(|m| !m.is_empty()) {
                if !menus.contains(&menu) {
                    warn(value_range(e, property), format!("attribute `{property}`: no `<ContextMenu x:Name=\"{menu}\">` in this view"));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Document;

    fn root(src: &str) -> Element {
        let parse = crate::syntax::parse(src);
        Document::cast(parse.syntax()).and_then(|d| d.root_element()).unwrap()
    }

    fn warnings_of(src: &str) -> Vec<String> {
        let parse = crate::syntax::parse(src);
        let root = Document::cast(parse.syntax()).and_then(|d| d.root_element()).unwrap();
        warnings(&parse, &root).into_iter().map(|d| d.message).collect()
    }

    #[test]
    fn the_language_server_checks_shortcuts_access_keys_and_references() {
        let w = warnings_of(
            r#"<Panel ContextMenu="nope">
  <Command x:Name="cmd_save" Shortcut="Ctrl+S"/>
  <MenuBar>
    <MenuItem Text="&amp;Fichier">
      <MenuItem Text="&amp;Enregistrer" ShortcutKeys="Ctrl+S"/>
      <MenuItem Text="&amp;Exporter" ShortcutKeys="Shift+E" Command="cmd_export"/>
      <MenuItem Text="Fermer" ShortcutKeys="Ctrl+Foo"/>
    </MenuItem>
    <MenuItem Text="&amp;Format"/>
  </MenuBar>
</Panel>"#,
        );
        assert!(w.contains(&"the shortcut `Ctrl+S` is already used on line 2".to_string()), "{w:?}");
        assert!(w.contains(&"attribute `ShortcutKeys`: `Shift+E` types text: add Ctrl or Alt, or use a function key".to_string()), "{w:?}");
        assert!(w.iter().any(|m| m.starts_with("attribute `ShortcutKeys`: `Foo` is not a key")), "{w:?}");
        assert!(w.contains(&"the access key `E` is already used by `Enregistrer` in this menu".to_string()), "{w:?}");
        assert!(w.contains(&"the access key `F` is already used by `Fichier` in this menu".to_string()), "{w:?}");
        assert!(w.contains(&"attribute `Command`: no `<Command x:Name=\"cmd_export\">` in this view".to_string()), "{w:?}");
        assert!(w.contains(&"attribute `ContextMenu`: no `<ContextMenu x:Name=\"nope\">` in this view".to_string()), "{w:?}");
        assert_eq!(w.len(), 7, "{w:?}");
    }

    /// The demo of every menu feature (`examples/views/menus.kbview`) compiles with no warning: every
    /// icon exists, every shortcut parses, no access key or shortcut is used twice.
    #[test]
    fn the_menus_demo_view_is_clean() {
        let src = include_str!("../examples/views/menus.kbview");
        let view = crate::compile::compile(src);
        assert!(view.is_ok(), "{:?}", view.err());
        let parse = crate::syntax::parse(src);
        let w: Vec<String> = crate::validate::warnings(&parse).into_iter().map(|d| d.message).collect();
        assert!(w.is_empty(), "{w:?}");
        let view = view.unwrap();
        assert_eq!(view.menus.len(), 7, "four menus of the bar, two buttons, the page menu");
        assert!(view.accelerators.len() >= 14);
    }

    #[test]
    fn references_accept_the_xaml_spellings() {
        assert_eq!(reference_name("menu1"), "menu1");
        assert_eq!(reference_name("{x:Ref menu1}"), "menu1");
        assert_eq!(reference_name("{x:Reference Name=menu1}"), "menu1");
        assert_eq!(reference_name("{Binding Menu}"), "");
    }

    #[test]
    fn menus_of_bars_and_buttons_are_read_with_their_commands() {
        let r = root(
            r#"<Panel>
                 <Command x:Name="cmd_save" Label="Enregistrer" SmallIcon="Save" Shortcut="Ctrl+S" OnExecute="save"/>
                 <MenuBar x:Name="bar">
                   <MenuItem Text="&amp;Fichier" OnDropDownOpening="file_opening">
                     <MenuItem Command="cmd_save"/>
                     <MenuSeparator/>
                     <MenuHeader Text="Récents"/>
                     <MenuItem Text="&amp;Quitter" ShortcutKeys="Alt+F4" OnClick="quit"/>
                   </MenuItem>
                   <MenuItem Text="&amp;Aide"/>
                 </MenuBar>
                 <DropDownButton Text="Nouveau"><MenuItem Text="Document" ShortcutKeys="Ctrl+N"/></DropDownButton>
                 <ContextMenu x:Name="edit"><MenuItem Text="Copier" ShortcutKeys="Ctrl+C" ShortcutKeyDisplayString="Ctrl+C (copier)"/></ContextMenu>
               </Panel>"#,
        );
        let menus = crate::window::read_menus(&r);
        let names: Vec<&str> = menus.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["#bar:1.0", "#bar:1.1", "#dd:2", "edit"]);
        let file = &menus[0];
        assert_eq!(file.opening.as_deref(), Some("file_opening"));
        assert_eq!(file.sender_element(), ("MenuItem", None));
        assert!(file.items[1].is_separator() && file.items[2].header);
        // The command gives the item its text, icon and shortcut.
        let vm = crate::binding::MapViewModel::new();
        let resolved = file.resolved(&vm, &HashMap::new());
        assert_eq!((resolved.items[0].text.as_str(), resolved.items[0].shortcut.as_str()), ("Enregistrer", "Ctrl+S"));
        assert_eq!(menus[3].items[0].shortcut_text(), "Ctrl+C (copier)");
        let accels = accelerators(&menus, &read_commands(&r), false);
        let keys: Vec<String> = accels.iter().map(|a| a.shortcut.to_string()).collect();
        assert_eq!(keys, ["Ctrl+S", "Alt+F4", "Ctrl+N", "Ctrl+C"]);
        assert!(matches(&accels[0].shortcut, vk::letter('S'), Modifiers::CTRL));
        assert!(!matches(&accels[0].shortcut, vk::letter('S'), Modifiers::CTRL_SHIFT));
        assert_eq!(accels[0].target, AcceleratorTarget::Item { menu: "#bar:1.0".into(), item: "1.0.0".into() });
    }

    #[test]
    fn a_command_no_item_runs_is_an_accelerator_only_without_a_ribbon() {
        let r = root(r#"<Panel><Command x:Name="cmd_find" Shortcut="Ctrl+F"/></Panel>"#);
        let commands = read_commands(&r);
        assert_eq!(accelerators(&[], &commands, false).len(), 1);
        assert!(accelerators(&[], &commands, true).is_empty());
    }

    #[test]
    fn item_templates_make_the_bound_items() {
        let r = root(
            r#"<Panel><ContextMenu x:Name="recent" ItemsSource="{Binding Recent}">
                 <ContextMenu.ItemTemplate><MenuItem Text="{Binding Name}" Icon="FileText" ToolTip="{Binding Path}"/></ContextMenu.ItemTemplate>
               </ContextMenu></Panel>"#,
        );
        let menu = crate::window::read_menus(&r).remove(0);
        let vm = crate::binding::MapViewModel::new().with(
            "Recent",
            crate::binding::Value::from(vec![crate::binding::Row::new().with("Name", crate::binding::Value::Str("a.kbdoc".into())).with("Path", crate::binding::Value::Str("C:/a.kbdoc".into()))]),
        );
        let resolved = menu.resolved(&vm, &HashMap::new());
        assert_eq!(resolved.items.len(), 1);
        let item = &resolved.items[0];
        assert_eq!((item.text.as_str(), item.tooltip.as_str(), item.icon.as_str()), ("a.kbdoc", "C:/a.kbdoc", "FileText"));
        assert_eq!(item.row_key.as_deref(), Some("a.kbdoc"));
    }
}
