//! Component family `navigation` — the navigation pane and the status bar of an application
//! window, over `kubuno_desktop_ui::navigation::Sidebar` and `kubuno_desktop_ui::navigation::StatusBar` (the
//! web shell's sidebar and the drive's status bar). Compiled with the `family-navigation` feature
//! (on by default through `all-families`).
//!
//! ## Elements
//!
//! * `<Sidebar>` — WinUI's `NavigationView` pane: `<SidebarItem>` rows (an icon and a label; a
//!   row with `<SidebarItem>` children is a tree group with a chevron), `<SidebarSection>`
//!   headers, or rows bound from `ItemsSource` (fields `Text`, `Icon`, `Key`, `Level`, and
//!   `Kind="Section"` for a header). `SelectedItem` (two-way) is the `Key` (else the `x:Name`,
//!   else the `Text`) of the active row; `OnItemInvoked` reports a click or Enter on a row.
//!   `DisplayMode="Compact"` shows the icon rail.
//! * `<StatusBar>` — WinForms' `StatusStrip`: `<StatusLabel>` cells, a `Spring` cell sharing
//!   the leftover width, a `Text="-"` cell drawn as a separator, a `Clickable` cell drawn as a
//!   ghost button raising its `OnClick`.

#[allow(unused_imports)] // Used by the `component!` invocations below.
use crate::registry::macros::component;
use crate::registry::ComponentMeta;

use crate::binding::{BindingMode, BindingSpec, ListWatch, PropSource, Value, ViewModel};
use crate::events::{ChangeSource, ItemEventArgs, TextChangedEventArgs};
use crate::node::{PaintCx, ViewEventKind, ViewNode};
use crate::props::{BuildError, Props};

use kubuno_desktop_controls::host;
use kubuno_desktop_controls::toolstrip::{StripItem, ToolStripItemDisplayStyle};
use kubuno_desktop_ui::navigation::{nav_item, section_item, status_item, NavKey, Sidebar, StatusBar, StripPaint};
use kubuno_desktop_ui::{Canvas, FocusId, Rect, Size};

/// A literal attribute, or a build error for a binding (structure is read once).
fn literal(props: &Props<'_>, name: &str, default: &str) -> Result<String, BuildError> {
    match props.str(name, default)? {
        PropSource::Literal(s) => Ok(s),
        PropSource::Bound { .. } => Err(BuildError::new(format!("attribute `{name}` must be a literal value, not a binding"), props.element().name_range())),
    }
}

/// The shared item block of a strip item, to change it.
fn item_mut(item: &mut StripItem) -> &mut kubuno_desktop_controls::toolstrip::ToolStripItem {
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

/// The Lucide glyph an `Icon="…"` names (the name itself when it is not one: nothing drawn).
fn glyph(name: &str) -> String {
    crate::icon::resolve(name).map(str::to_string).unwrap_or_else(|| name.to_string())
}

// ═════════════════════════════════════════════════════════════════════════
// Sidebar
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: sidebar_item,
    name: "SidebarItem",
    // Note: One row of a `<Sidebar>`; `<SidebarItem>` children make it a tree group.
    doc: "A row of a Sidebar: an icon and a label. SidebarItem children make it a group the user can expand or collapse.",
    ctor: kubuno_desktop_ui::navigation::nav_item("Folder", "Documents", false, 0.0),
    children: ChildrenModel::List(&["SidebarItem"]),
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Label of the row.").category("Appearance").localizable().bindable(),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon shown before the label: a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.").editor("icon").category("Icon"),
        PropertyMeta::new("Key", PropKind::String, "", "What SelectedItem holds when the row is active; empty for the x:Name, else the text.").category("Data"),
        PropertyMeta::new("Expanded", PropKind::Bool, "false", "For a row with children: whether they are shown at first.").category("Behavior"),
        PropertyMeta::new("Indent", PropKind::F32, "-1", "Where its icon starts, in pixels from the row's left; -1 for one step per tree level (plus the chevron's column for a group). A flat row set to a group's indent lines up with the groups.").category("Layout"),
        PropertyMeta::new("Enabled", PropKind::Bool, "true", "Whether the row can be chosen.").category("Behavior").bindable(),
        PropertyMeta::new("Visible", PropKind::Bool, "true", "Whether the row is shown.").category("Behavior").bindable(),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the row is chosen.").category(crate::registry::EventCategory::Action).args::<crate::events::ItemEventArgs>(),
    ],
    smoke: |i| { i },
    build: |props, _cx| {
        Err(BuildError::new("`<SidebarItem>` is only valid inside `<Sidebar>` or another `<SidebarItem>`", props.element().name_range()))
    },
}

component! {
    mod_name: sidebar_section,
    name: "SidebarSection",
    // Note: A header row of a `<Sidebar>`.
    doc: "A section header of a Sidebar: small uppercase text above the rows that follow it.",
    ctor: kubuno_desktop_ui::navigation::section_item("", "Récents", 0.0),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text of the header.").category("Appearance").localizable().bindable(),
        PropertyMeta::new("Visible", PropKind::Bool, "true", "Whether the header is shown.").category("Behavior").bindable(),
    ],
    events: [],
    smoke: |i| { i },
    build: |props, _cx| {
        Err(BuildError::new("`<SidebarSection>` is only valid inside `<Sidebar>`", props.element().name_range()))
    },
}

component! {
    mod_name: sidebar,
    name: "Sidebar",
    // Note: The navigation pane (`kubuno_desktop_ui::navigation::Sidebar`, WinUI `NavigationView`).
    doc: "A navigation pane: rows with an icon and a label, section headers and expandable groups. The active row is SelectedItem.",
    ctor: kubuno_desktop_ui::navigation::Sidebar::new(),
    children: ChildrenModel::List(&["SidebarItem", "SidebarSection"]),
    default_event: "OnItemInvoked",
    props: [
        PropertyMeta::new("SelectedItem", PropKind::String, "", "The Key of the active row (its x:Name or its text when it has no Key).").category("Behavior").bindable(),
        PropertyMeta::new("DisplayMode", PropKind::Enum(&["Expanded", "Compact"]), "Expanded", "Expanded shows the labels; Compact shows a rail of icons.").category("Appearance").bindable(),
        PropertyMeta::new("ItemsSource", PropKind::String, "", "Rows from a list (fields Text, Icon, Key, Level, Expanded, Enabled, Indent in pixels, and Kind=\"Section\" for a header), instead of the rows written inside.").category("Data").bindable().editor("list"),
    ],
    events: [
        EventMeta::new("OnItemInvoked", "Occurs when a row is chosen (clicked, or Enter): its key is the new text.").category(crate::registry::EventCategory::Action).args::<crate::events::TextChangedEventArgs>(),
        EventMeta::new("OnSelectionChanged", "Occurs when the active row changes.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>(),
    ],
    smoke: |s| { s.with(kubuno_desktop_ui::navigation::nav_item("Folder", "Documents", true, 0.0), None) },
    build: |props, _cx| {
        crate::registry::families::navigation::build_sidebar(props)
    },
}

/// One row of a `<Sidebar>`.
#[derive(Clone)]
struct NavRow {
    section: bool,
    key: String,
    text: PropSource<String>,
    icon: String,
    level: usize,
    /// Its indent in DIP, when the list sets one (`Indent`): rows of a flat list that line up with
    /// the icons of a tree's groups. `None`: one step per level, plus the chevron's column for a group.
    indent: Option<f32>,
    /// The row that holds it (a tree group).
    parent: Option<usize>,
    has_children: bool,
    enabled: PropSource<bool>,
    visible: PropSource<bool>,
    handler: Option<String>,
}

fn read_rows(element: &crate::ast::Element, level: usize, parent: Option<usize>, out: &mut Vec<NavRow>, expanded: &mut Vec<bool>) -> Result<(), BuildError> {
    for child in element.children() {
        let name = child.name().unwrap_or_default();
        let meta = crate::registry::lookup(&name).ok_or_else(|| BuildError::new(format!("`<{name}>` is not registered"), child.name_range()))?;
        let p = Props::new(&child, meta);
        match name.as_str() {
            "SidebarSection" => {
                out.push(NavRow {
                    section: true,
                    key: String::new(),
                    text: p.str("Text", "")?,
                    icon: String::new(),
                    level,
                    parent,
                    indent: None,
                    has_children: false,
                    enabled: PropSource::Literal(true),
                    visible: p.bool("Visible", true)?,
                    handler: None,
                });
                expanded.push(false);
            }
            "SidebarItem" => {
                let text = p.str("Text", "")?;
                let x_name = child.attribute("x:Name").and_then(|a| a.value()).filter(|v| !v.is_empty());
                let key = literal(&p, "Key", "")?;
                let key = if !key.is_empty() {
                    key
                } else if let Some(n) = x_name {
                    n
                } else if let PropSource::Literal(t) = &text {
                    t.clone()
                } else {
                    format!("item{}", out.len() + 1)
                };
                let index = out.len();
                let has_children = child.children().next().is_some();
                out.push(NavRow {
                    section: false,
                    key,
                    text,
                    icon: glyph(&literal(&p, "Icon", "")?),
                    level,
                    parent,
                    indent: literal(&p, "Indent", "")?.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0),
                    has_children,
                    enabled: p.bool("Enabled", true)?,
                    visible: p.bool("Visible", true)?,
                    handler: p.event("OnClick"),
                });
                expanded.push(matches!(p.bool("Expanded", false)?, PropSource::Literal(true)));
                if has_children {
                    read_rows(&child, level + 1, Some(index), out, expanded)?;
                }
            }
            other => return Err(BuildError::new(format!("`<Sidebar>` accepts `<SidebarItem>` and `<SidebarSection>` children, found `<{other}>`"), child.name_range())),
        }
    }
    Ok(())
}

/// Reads `<Sidebar>`.
pub(crate) fn build_sidebar(props: &Props<'_>) -> Result<Box<dyn ViewNode>, BuildError> {
    let mut rows = Vec::new();
    let mut expanded = Vec::new();
    read_rows(props.element(), 0, None, &mut rows, &mut expanded)?;
    Ok(Box::new(SidebarNode {
        rows,
        expanded,
        selected: props.str("SelectedItem", "")?,
        mode: props.enum_("DisplayMode", "Expanded")?,
        items_source: props.str("ItemsSource", "")?.binding().cloned(),
        watch: ListWatch::default(),
        bound: None,
        on_invoked: props.event("OnItemInvoked"),
        on_changed: props.event("OnSelectionChanged"),
        focus_id: props.focus_id(),
        scroll: 0.0,
        pressed: None,
        focused_row: None,
        current: None,
    }))
}

/// `<Sidebar>`'s live node.
pub struct SidebarNode {
    rows: Vec<NavRow>,
    /// Whether each tree group shows its children (index-aligned with `rows`).
    expanded: Vec<bool>,
    selected: PropSource<String>,
    mode: PropSource<String>,
    items_source: Option<BindingSpec>,
    watch: ListWatch,
    /// Rows read from `ItemsSource` (replace the written ones while the list resolves).
    bound: Option<(Vec<NavRow>, Vec<bool>)>,
    on_invoked: Option<String>,
    on_changed: Option<String>,
    focus_id: Option<FocusId>,
    scroll: f32,
    pressed: Option<usize>,
    focused_row: Option<usize>,
    /// The active key as last shown (a click changes it before the binding follows).
    current: Option<String>,
}

impl SidebarNode {
    fn rows_from_list(list: &[crate::binding::Row]) -> (Vec<NavRow>, Vec<bool>) {
        let mut rows = Vec::with_capacity(list.len());
        let mut parents: Vec<usize> = Vec::new();
        for (i, r) in list.iter().enumerate() {
            let level = r.text("Level").parse::<usize>().unwrap_or(0);
            parents.truncate(level);
            let parent = parents.last().copied();
            if let Some(p) = parent {
                if let Some(row) = rows.get_mut(p) {
                    let row: &mut NavRow = row;
                    row.has_children = true;
                }
            }
            let text = r.text("Text");
            let key = Some(r.text("Key")).filter(|k| !k.is_empty()).unwrap_or_else(|| text.clone());
            let section = r.text("Kind") == "Section";
            rows.push(NavRow {
                section,
                key,
                text: PropSource::Literal(text),
                icon: glyph(&r.text("Icon")),
                level,
                parent,
                indent: r.text("Indent").trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0),
                has_children: false,
                enabled: PropSource::Literal(r.text("Enabled") != "false"),
                visible: PropSource::Literal(true),
                handler: None,
            });
            if !section {
                parents.resize(level, i);
                parents.push(i);
            }
        }
        let expanded = list.iter().map(|r| r.text("Expanded") == "true").collect();
        (rows, expanded)
    }

    /// Whether row `i` is shown: visible, and every group holding it expanded.
    fn shown(rows: &[NavRow], expanded: &[bool], vm: &dyn ViewModel, i: usize) -> bool {
        let mut at = Some(i);
        let mut first = true;
        while let Some(j) = at {
            let row = &rows[j];
            if !row.visible.resolve(vm) || (!first && !expanded.get(j).copied().unwrap_or(false)) {
                return false;
            }
            first = false;
            at = row.parent;
        }
        true
    }

    /// The `kubuno_desktop_ui` pane for this frame.
    fn pane(rows: &[NavRow], expanded: &[bool], vm: &dyn ViewModel, active: &str, compact: bool, scroll: f32) -> Sidebar {
        let mut s = Sidebar::new();
        s.mode = if compact { kubuno_drive_desktop_app_controls::sidebar::SidebarMode::Compact } else { kubuno_drive_desktop_app_controls::sidebar::SidebarMode::Expanded };
        s.scroll_y = scroll;
        for (i, row) in rows.iter().enumerate() {
            let indent = row.indent.unwrap_or(row.level as f32 * INDENT_PER_LEVEL + if row.has_children { INDENT_PER_LEVEL } else { 0.0 });
            let text = row.text.resolve(vm);
            let mut item = if row.section { section_item(&row.icon, &text, indent) } else { nav_item(&row.icon, &text, !row.key.is_empty() && row.key == active, indent) };
            {
                let it = item_mut(&mut item);
                it.visible = Self::shown(rows, expanded, vm, i);
                it.enabled = row.enabled.resolve(vm);
                if compact {
                    it.tool_tip_text = text.clone();
                }
            }
            let chevron = (row.has_children && !row.section).then(|| expanded.get(i).copied().unwrap_or(false));
            s = s.with(item, chevron);
        }
        s
    }
}

/// The indent of one tree level (`kubuno_drive_desktop_app_controls::sidebar::INDENT_PER_LEVEL`).
const INDENT_PER_LEVEL: f32 = 16.0;

impl ViewNode for SidebarNode {
    fn measure(&self, _c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let compact = self.mode.resolve(vm) == "Compact";
        let (rows, expanded) = match &self.bound {
            Some((r, e)) => (r.as_slice(), e.as_slice()),
            None => (self.rows.as_slice(), self.expanded.as_slice()),
        };
        let s = Self::pane(rows, expanded, vm, "", compact, 0.0);
        Size::new(s.pane_width(), s.content_height())
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let design = cx.design.is_some();
        if let Some(list) = self.watch.changed(&*cx.vm, self.items_source.as_ref()) {
            self.bound = Some(Self::rows_from_list(&list));
        }
        let wanted = self.selected.resolve(&*cx.vm);
        if self.current.as_deref() != Some(wanted.as_str()) && self.selected.binding().is_none_or(|s| s.mode != BindingMode::TwoWay || self.current.is_none() || !wanted.is_empty()) {
            self.current = Some(wanted);
        }
        let active = self.current.clone().unwrap_or_default();
        let compact = self.mode.resolve(&*cx.vm) == "Compact";
        let (rows, expanded) = match &self.bound {
            Some((r, e)) => (r.clone(), e.clone()),
            None => (self.rows.clone(), self.expanded.clone()),
        };
        let canvas: &dyn Canvas = cx.canvas;
        let mut pane = Self::pane(&rows, &expanded, &*cx.vm, &active, compact, self.scroll);
        self.scroll = self.scroll.clamp(0.0, pane.max_scroll(bounds));
        pane.scroll_y = self.scroll;

        let frame = cx.frame;
        let (mx, my) = frame.mouse;
        let inside = !design && !frame.pointer_outside() && bounds.contains(mx, my);
        let hot = if inside { pane.item_at(bounds, mx, my).filter(|&i| !rows[i].section) } else { None };
        let focus = self.focus_id.map(|id| cx.focus.register(id, bounds)).unwrap_or_default();

        // Wheel.
        if inside {
            let (_, wy) = frame.wheel_dip();
            let max = pane.max_scroll(bounds);
            if wy != 0.0 && max > 0.0 && !host::wheel_claimed() {
                let before = self.scroll;
                self.scroll = (self.scroll + wy).clamp(0.0, max);
                if self.scroll != before {
                    host::claim_wheel();
                    pane.scroll_y = self.scroll;
                }
            }
        }

        // Keyboard, while the pane holds the focus: arrows move, Enter invokes.
        let mut invoke: Option<usize> = None;
        if focus.focused && !design {
            for key in kubuno_desktop_ui::navigation::take_nav_keys(true) {
                let from = self.focused_row.or_else(|| rows.iter().position(|r| !r.section && r.key == active));
                self.focused_row = pane.step_focus(from, key);
                if let (Some(i), NavKey::Next | NavKey::Prev) = (self.focused_row, key) {
                    self.scroll = pane.reveal_row(bounds, i);
                }
            }
            if kubuno_desktop_ui::navigation::take_activate() {
                invoke = self.focused_row;
            }
        }

        // Clicks.
        if !design {
            if frame.mouse_down && inside && self.pressed.is_none() {
                self.pressed = hot.or(Some(usize::MAX));
            }
            if !frame.mouse_down {
                if let (Some(p), Some(h)) = (self.pressed, hot) {
                    if p == h {
                        invoke = Some(h);
                    }
                }
                self.pressed = None;
            }
        }

        let paint = StripPaint {
            hot,
            pressed: self.pressed.filter(|p| *p != usize::MAX),
            focused: self.focused_row.filter(|_| focus.focused),
            focus_visible: focus.visible,
            ..StripPaint::hot(hot)
        };
        pane.paint_with(canvas, bounds, &paint);

        // Its rows in the accessibility tree, under this element: a header as a text, a row as a list
        // item named by its label (its tooltip in the compact rail says the same).
        if let (false, Some((slot, _)), Some(services)) = (design, cx.sender.clone(), cx.services.as_deref_mut()) {
            use kubuno_desktop_controls::host::access::{AccessNode, AccessRole};
            let parent = crate::common::access_id(&slot.id);
            for (i, rect) in pane.row_rects(bounds).into_iter().enumerate() {
                let Some(row) = rows.get(i) else { continue };
                if !Self::shown(&rows, &expanded, &*cx.vm, i) || rect.bottom <= bounds.top || rect.top >= bounds.bottom {
                    continue;
                }
                let c = crate::common::to_client(rect);
                services.access.push(AccessNode {
                    id: crate::common::access_id(&format!("{}/row{i}", slot.id)),
                    parent: Some(parent),
                    role: if row.section { AccessRole::Label } else if row.has_children { AccessRole::TreeItem } else { AccessRole::ListItem },
                    name: row.text.resolve(&*cx.vm),
                    description: String::new(),
                    value: None,
                    bounds: (c.left, c.top, c.right, c.bottom),
                    focusable: false,
                    disabled: !row.enabled.resolve(&*cx.vm),
                    checked: None,
                    clickable: false,
                    read_only: true,
                    access_key: None,
                    expanded: None,
                });
            }
        }

        let Some(i) = invoke.filter(|&i| i < rows.len() && rows[i].enabled.resolve(&*cx.vm)) else { return };
        let row = &rows[i];
        if row.has_children {
            // A group toggles its children.
            let target = if self.bound.is_some() { self.bound.as_mut().map(|(_, e)| e) } else { Some(&mut self.expanded) };
            if let Some(e) = target.and_then(|e| e.get_mut(i)) {
                *e = !*e;
            }
            host::request_repaint_after(0);
            return;
        }
        let key = row.key.clone();
        if let Some(h) = row.handler.as_deref() {
            cx.fire("OnClick", self.focus_id, Some(h), ViewEventKind::Clicked, &mut ItemEventArgs { index: i });
        }
        let old = active.clone();
        let mut args = TextChangedEventArgs::new(old.clone(), key.clone(), ChangeSource::User);
        cx.fire("OnItemInvoked", self.focus_id, self.on_invoked.as_deref(), ViewEventKind::Changed(key.clone()), &mut args);
        if old != key {
            self.current = Some(key.clone());
            if let Some(spec) = self.selected.binding().filter(|s| s.mode.writes_back()) {
                spec.update_source(cx.vm, Value::Str(key.clone()));
            }
            let mut args = TextChangedEventArgs::new(old, key.clone(), ChangeSource::User);
            cx.fire("OnSelectionChanged", self.focus_id, self.on_changed.as_deref(), ViewEventKind::Changed(key), &mut args);
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// StatusBar
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: status_label,
    name: "StatusLabel",
    // Note: One cell of a `<StatusBar>` (`ToolStripStatusLabel`).
    doc: "A cell of a StatusBar: a text, an optional icon. Spring makes it share the leftover width; a Text of a single dash makes a separator; Clickable makes it a button.",
    ctor: kubuno_desktop_ui::navigation::status_item("Prêt", true),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text of the cell. A single dash makes a separator.").category("Appearance").localizable().bindable(),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon shown before the text (a clickable cell): a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.").editor("icon").category("Icon"),
        PropertyMeta::new("ForeColor", PropKind::String, "", "Colour of the cell's text (a theme colour such as Caution, or #RRGGBB); empty: the status bar's secondary text colour.").category("Appearance").bindable().editor("color").type_converter("Color"),
        PropertyMeta::new("Spring", PropKind::Bool, "false", "The cell takes the width the other cells leave.").category("Layout"),
        PropertyMeta::new("Clickable", PropKind::Bool, "false", "The cell is a button: it lights up under the mouse and raises OnClick.").category("Behavior"),
        PropertyMeta::new("Enabled", PropKind::Bool, "true", "Whether a clickable cell can be clicked.").category("Behavior").bindable(),
        PropertyMeta::new("Visible", PropKind::Bool, "true", "Whether the cell is shown.").category("Behavior").bindable(),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when a clickable cell is clicked.").category(crate::registry::EventCategory::Action).args::<crate::events::ItemEventArgs>(),
    ],
    smoke: |i| { i },
    build: |props, _cx| {
        Err(BuildError::new("`<StatusLabel>` is only valid inside `<StatusBar>`", props.element().name_range()))
    },
}

component! {
    mod_name: status_bar,
    name: "StatusBar",
    // Note: The status bar (`kubuno_desktop_ui::navigation::StatusBar`, WinForms `StatusStrip`).
    doc: "A status bar: a row of StatusLabel cells along the bottom of a window.",
    ctor: kubuno_desktop_ui::navigation::StatusBar::new(),
    children: ChildrenModel::List(&["StatusLabel"]),
    default_event: "OnItemClicked",
    props: [],
    events: [
        EventMeta::new("OnItemClicked", "Occurs when a clickable cell is clicked (its index is in the event).").category(crate::registry::EventCategory::Action).args::<crate::events::ItemEventArgs>(),
    ],
    smoke: |s| { s.with(kubuno_desktop_ui::navigation::status_item("Prêt", true)) },
    build: |props, _cx| {
        crate::registry::families::navigation::build_status_bar(props)
    },
}

struct StatusCell {
    text: PropSource<String>,
    /// `ForeColor`: the cell's ink (a theme colour such as Caution, #RRGGBB…); empty for the bar's.
    fore_color: PropSource<String>,
    icon: String,
    spring: bool,
    clickable: bool,
    enabled: PropSource<bool>,
    visible: PropSource<bool>,
    handler: Option<String>,
}

pub(crate) fn build_status_bar(props: &Props<'_>) -> Result<Box<dyn ViewNode>, BuildError> {
    let mut cells = Vec::new();
    for child in props.element().children() {
        let name = child.name().unwrap_or_default();
        if name != "StatusLabel" {
            return Err(BuildError::new(format!("`<StatusBar>` accepts `<StatusLabel>` children, found `<{name}>`"), child.name_range()));
        }
        let meta = crate::registry::lookup("StatusLabel").ok_or_else(|| BuildError::new("`StatusLabel` is not registered", child.name_range()))?;
        let p = Props::new(&child, meta);
        cells.push(StatusCell {
            text: p.str("Text", "")?,
            fore_color: p.str("ForeColor", "")?,
            icon: glyph(&literal(&p, "Icon", "")?),
            spring: matches!(p.bool("Spring", false)?, PropSource::Literal(true)),
            clickable: matches!(p.bool("Clickable", false)?, PropSource::Literal(true)),
            enabled: p.bool("Enabled", true)?,
            visible: p.bool("Visible", true)?,
            handler: p.event("OnClick"),
        });
    }
    Ok(Box::new(StatusBarNode { cells, on_clicked: props.event("OnItemClicked"), focus_id: props.focus_id(), pressed: None }))
}

/// `<StatusBar>`'s live node.
pub struct StatusBarNode {
    cells: Vec<StatusCell>,
    on_clicked: Option<String>,
    focus_id: Option<FocusId>,
    pressed: Option<usize>,
}

impl StatusBarNode {
    fn bar(&self, vm: &dyn ViewModel, theme: &kubuno_desktop_ui::Theme) -> StatusBar {
        let mut bar = StatusBar::new();
        for cell in &self.cells {
            let text = cell.text.resolve(vm);
            let mut item = if text.trim() == "-" {
                kubuno_desktop_ui::navigation::separator_item()
            } else if cell.clickable {
                let mut b = kubuno_desktop_controls::toolstrip::ToolStripButton::default();
                b.item.text = text;
                if !cell.icon.is_empty() {
                    b.item.image = Some(cell.icon.clone());
                    b.item.display_style = ToolStripItemDisplayStyle::ImageAndText;
                }
                StripItem::Button(b)
            } else {
                status_item(&text, cell.spring)
            };
            item_mut(&mut item).visible = cell.visible.resolve(vm);
            item_mut(&mut item).enabled = cell.enabled.resolve(vm);
            item_mut(&mut item).fore_color = crate::style::parse_color(&cell.fore_color.resolve(vm)).ok().flatten().map(|c| c.resolve_with(theme, crate::style::high_contrast()));
            bar = bar.with(item);
        }
        bar
    }
}

impl ViewNode for StatusBarNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn ViewModel) -> Size {
        let widths: f32 = self.bar(vm, c.theme()).item_widths(c).iter().sum();
        Size::new(widths.max(200.0), 24.0)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let bar = self.bar(&*cx.vm, cx.canvas.theme());
        let canvas: &dyn Canvas = cx.canvas;
        let frame = cx.frame;
        let design = cx.design.is_some();
        let (mx, my) = frame.mouse;
        let inside = !design && !frame.pointer_outside() && bounds.contains(mx, my);
        let hot = if inside { bar.item_at(canvas, bounds, mx, my).filter(|&i| self.cells.get(i).is_some_and(|c| c.clickable) && bar.is_focusable(i)) } else { None };
        let mut clicked = None;
        if !design {
            if frame.mouse_down && self.pressed.is_none() && inside {
                self.pressed = hot.or(Some(usize::MAX));
            }
            if !frame.mouse_down {
                if let (Some(p), Some(h)) = (self.pressed, hot) {
                    if p == h {
                        clicked = Some(h);
                    }
                }
                self.pressed = None;
            }
        }
        let paint = StripPaint { hot, pressed: self.pressed.filter(|p| *p != usize::MAX), ..StripPaint::hot(hot) };
        bar.paint_with(canvas, bounds, &paint);
        if let Some(i) = clicked {
            let handler = self.cells[i].handler.clone();
            if let Some(h) = handler.as_deref() {
                cx.fire("OnClick", self.focus_id, Some(h), ViewEventKind::Clicked, &mut ItemEventArgs { index: i });
            }
            cx.fire("OnItemClicked", self.focus_id, self.on_clicked.as_deref(), ViewEventKind::Clicked, &mut ItemEventArgs { index: i });
        }
    }
}

/// Every component this family declares.
pub const ALL: &[ComponentMeta] = &[sidebar::META, sidebar_item::META, sidebar_section::META, status_bar::META, status_label::META];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::binding::{MapViewModel, Row};
    use crate::runtime::Runtime;
    use kubuno_desktop_controls::host::{Frame, Modifiers};
    use kubuno_desktop_ui::graphics::testing::RecordingCanvas;

    fn frame(mouse: Option<(f32, f32)>, down: bool) -> Frame {
        let (x, y) = mouse.unwrap_or((host::POINTER_AWAY, host::POINTER_AWAY));
        Frame {
            size: (240.0, 400.0),
            mouse: (x, y),
            mouse_down: down,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 240.0, 400.0),
            chrome_top: 0.0,
            mods: Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: 1,
            window_focused: true,
        }
    }

    fn run(rt: &mut Runtime, vm: &mut MapViewModel, f: Frame) -> RecordingCanvas {
        let canvas = RecordingCanvas::new();
        let mut handlers = crate::binding::HandlerTable::new();
        rt.frame(&canvas, &f, vm, &mut handlers, Rect::new(0.0, 0.0, 240.0, 400.0));
        canvas
    }

    const VIEW: &str = r#"<Panel DesignWidth="240" DesignHeight="400">
  <Sidebar x:Name="nav" SelectedItem="{Binding Page, Mode=TwoWay}" Dock="Fill">
    <SidebarSection Text="Espace"/>
    <SidebarItem x:Name="home" Text="Accueil" Icon="Home"/>
    <SidebarItem Text="Dossiers" Icon="Folder" Expanded="false">
      <SidebarItem Key="work" Text="Travail" Icon="Briefcase"/>
    </SidebarItem>
  </Sidebar>
</Panel>"#;

    fn texts(c: &RecordingCanvas) -> Vec<String> {
        c.calls().into_iter().filter(|s| s.starts_with("text(")).collect()
    }

    #[test]
    fn a_click_selects_a_row_and_a_group_expands() {
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(VIEW), "{:?}", rt.diagnostics());
        let mut vm = MapViewModel::new().with("Page", Value::Str(String::new()));
        let c = run(&mut rt, &mut vm, frame(None, false));
        assert!(!texts(&c).iter().any(|t| t.contains("Travail")), "the group starts collapsed");
        // The rows: the section (with its gap), then `Accueil`, then `Dossiers`.
        let (row_home, row_group) = {
            let pane = SidebarNode::pane(&[], &[], &vm, "", false, 0.0);
            let _ = pane;
            (60.0, 100.0)
        };
        let home_y = texts(&c).iter().find(|t| t.contains("Accueil")).and_then(|t| t.split(' ').nth(1)).and_then(|r| r.split(',').nth(1)).and_then(|v| v.parse::<f32>().ok()).unwrap_or(row_home);
        run(&mut rt, &mut vm, frame(Some((100.0, home_y + 8.0)), true));
        run(&mut rt, &mut vm, frame(Some((100.0, home_y + 8.0)), false));
        assert_eq!(vm.get("Page"), Some(Value::Str("home".into())));
        let group_y = texts(&c).iter().find(|t| t.contains("Dossiers")).and_then(|t| t.split(' ').nth(1)).and_then(|r| r.split(',').nth(1)).and_then(|v| v.parse::<f32>().ok()).unwrap_or(row_group);
        run(&mut rt, &mut vm, frame(Some((100.0, group_y + 8.0)), true));
        run(&mut rt, &mut vm, frame(Some((100.0, group_y + 8.0)), false));
        let c = run(&mut rt, &mut vm, frame(None, false));
        assert!(texts(&c).iter().any(|t| t.contains("Travail")), "the group expanded: {:?}", texts(&c));
    }

    #[test]
    fn rows_can_come_from_a_list() {
        let list = vec![
            Row::new().with("Text", Value::Str("Récents".into())).with("Kind", Value::Str("Section".into())),
            Row::new().with("Text", Value::Str("Mail".into())).with("Icon", Value::Str("Mail".into())).with("Key", Value::Str("mail".into())),
            Row::new().with("Text", Value::Str("Sous".into())).with("Level", Value::F32(1.0)),
        ];
        let (rows, _) = SidebarNode::rows_from_list(&list);
        assert!(rows[0].section);
        assert_eq!(rows[1].key, "mail");
        assert!(rows[1].has_children);
        assert_eq!(rows[2].parent, Some(1));
    }

    #[test]
    fn a_status_bar_shows_its_cells_and_clicks_a_clickable_one() {
        let view = r#"<Panel DesignWidth="400" DesignHeight="24"><StatusBar Dock="Fill" OnItemClicked="clicked"><StatusLabel Text="{Binding Status}" Spring="true"/><StatusLabel Text="-"/><StatusLabel Text="main" Icon="GitBranch" Clickable="true"/></StatusBar></Panel>"#;
        let mut rt = Runtime::new();
        assert!(rt.reload_from_text(view), "{:?}", rt.diagnostics());
        let mut vm = MapViewModel::new().with("Status", Value::Str("12 éléments".into()));
        let canvas = RecordingCanvas::new();
        let mut handlers = crate::binding::HandlerTable::new();
        rt.frame(&canvas, &frame(None, false), &mut vm, &mut handlers, Rect::new(0.0, 0.0, 400.0, 24.0));
        assert!(texts(&canvas).iter().any(|t| t.contains("12 éléments")));
        assert!(texts(&canvas).iter().any(|t| t.contains("main")));
    }
}
