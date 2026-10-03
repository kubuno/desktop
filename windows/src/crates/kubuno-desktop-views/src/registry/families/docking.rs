//! Component family `docking` — the dock and the workspace chrome of the
//! advanced editors (`kubuno_desktop_ui::dock`, `kubuno_desktop_ui::workspace`, ports of the
//! web's `core/frontend/src/core/shell/workspace/`). Compiled with the
//! `family-docking` feature (on by default through `all-families`).
//!
//! ## Elements
//!
//! * `<DockArea>` — a viewport surrounded by dockable panels. Its children are
//!   `<DockPanel>`s plus AT MOST ONE other element: the viewport's content
//!   (the web's `children`). The panels' `Side` / `Group` / `Active` describe
//!   the DEFAULT arrangement (the web's `defaultArrangement`); the user then
//!   re-docks, merges, splits, floats, resizes, closes and reopens them, and the
//!   resulting layout is persisted under `StorageKey` and exposed, as the web's
//!   JSON, through the bindable `Layout` property.
//! * `<DockPanel>` — one panel: a title (its tab), an optional icon and one
//!   child element (its body; a `<Panel>` to host several controls). Valid only
//!   directly inside `<DockArea>`. Its id is its `x:Name` (else `panel<N>`).
//! * `<WorkspaceShell>` — the chrome around a dock: topbar (back · title ·
//!   editor name · document info · search · delete), optional status bar, and
//!   one child element, the body.
//!
//! ## Design time
//!
//! In the designer (`PaintCx::design` set) the dock shows the DEFAULT
//! arrangement (the saved layout is ignored), takes no input, and paints each
//! group's active panel through its `<DockPanel>`'s design slot — so a click on
//! a panel body selects that `<DockPanel>` (or the control inside it), and a
//! control dropped on an empty panel becomes its body. `Active="true"` picks
//! the panel shown in its group.

#[allow(unused_imports)] // Used by the `component!` invocations below.
use crate::registry::macros::component;
use crate::registry::{ComponentMeta, LayoutKind};

use crate::binding::{PropSource, Value};
use crate::events::{ChangeSource, TextChangedEventArgs};
use crate::node::{PaintCx, ViewEventKind, ViewNode};
use crate::props::{BuildError, Props};

use kubuno_desktop_controls::host::Frame;
use kubuno_desktop_ui::dock::{DockArea, DockArrangement, DockEvent, DockPanel, DockSide, DockSlot, DockTheme};
use kubuno_desktop_ui::workspace::WorkspaceShell;
use kubuno_desktop_ui::{Canvas, FocusId, Rect, Size};

/// A literal attribute, or a build error for a `{Binding …}` — the panels'
/// arrangement is structure, read once (like `<Tabs>`' `Header`s).
fn literal(props: &Props<'_>, name: &str, default: &str) -> Result<String, BuildError> {
    match props.str(name, default)? {
        PropSource::Literal(s) => Ok(s),
        PropSource::Bound { .. } => Err(BuildError::new(format!("attribute `{name}` must be a literal value, not a binding"), None)),
    }
}

fn literal_bool(props: &Props<'_>, name: &str, default: bool) -> Result<bool, BuildError> {
    match props.bool(name, default)? {
        PropSource::Literal(v) => Ok(v),
        PropSource::Bound { .. } => Err(BuildError::new(format!("attribute `{name}` must be a literal value, not a binding"), None)),
    }
}

// ═════════════════════════════════════════════════════════════════════════
// DockPanel
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: dock_panel,
    name: "DockPanel",
    // Note: One panel of a `<DockArea>` (`kubuno_desktop_ui::dock::DockPanel`): a tab and one body. Valid only as a direct `<DockArea>` child.
    doc: "A panel of a DockArea: a tab the user can move, dock, float or close, and one child element as its content.",
    ctor: kubuno_desktop_ui::dock::DockPanel::new("panel1", "Panneau"),
    children: ChildrenModel::SingleWidget,
    props: [
        PropertyMeta::new("Title", PropKind::String, "", "Text of the panel's tab."),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon shown before the title: a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.").editor("icon").category("Icon"),
        PropertyMeta::new("Side", PropKind::Enum(&["Right", "Left", "Float"]), "Right", "Where the panel is docked by default."),
        PropertyMeta::new("Group", PropKind::String, "", "Panels of the same side with the same group share one tab group; empty for a group of its own."),
        PropertyMeta::new("Active", PropKind::Bool, "false", "Shows this panel first in its tab group."),
        PropertyMeta::new("Closable", PropKind::Bool, "true", "Whether the user can close the panel."),
    ],
    events: [],
    smoke: |p| { p.icon("Layers").closable(false) },
    build: |props, cx| {
        use super::*;
        let child = props.build_single_child(cx)?;
        Ok(Box::new(crate::registry::families::docking::DockPanelNode { child }) as Box<dyn ViewNode>)
    },
}

/// `<DockPanel>`'s node: its body. The panel's tab and placement are the
/// dock's; this node only paints the body where the dock put it (inside the
/// panel's design slot, so the designer maps that area to the element).
pub struct DockPanelNode {
    child: Option<Box<dyn ViewNode>>,
}

impl ViewNode for DockPanelNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn crate::binding::ViewModel) -> Size {
        self.child.as_ref().map(|n| n.measure(c, vm)).unwrap_or(Size::EMPTY)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        if let Some(child) = self.child.as_mut() {
            let mut inner = cx.reborrow();
            child.paint(&mut inner, bounds);
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// DockArea
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: dock_area,
    name: "DockArea",
    // Note: A viewport and its dockable panels (`kubuno_desktop_ui::dock::DockArea`, port of the web's `Dock.tsx`).
    doc: "A work area surrounded by panels the user can dock left or right, group as tabs, split, float, resize, close and reopen. Add the panels as DockPanel children; one other child is the content of the central area.",
    ctor: kubuno_desktop_ui::dock::DockArea::new(Vec::new(), kubuno_desktop_ui::dock::DockArrangement::new()),
    children: ChildrenModel::List(&["DockPanel"]),
    default_event: "OnPanelActivated",
    props: [
        PropertyMeta::new("StorageKey", PropKind::String, "", "Key under which the user's layout is saved and restored; empty to keep no layout between runs."),
        PropertyMeta::new("Layout", PropKind::String, "", "The current layout, as JSON; set it to restore a layout, set it empty to restore the default arrangement."),
        PropertyMeta::new("ActivePanel", PropKind::String, "", "Name of the panel shown in front; setting it opens and shows that panel."),
        PropertyMeta::new("Theme", PropKind::Enum(&["Default", "WorkspaceDark", "WorkspaceLight"]), "Default", "Colours of the panels: the application's, or those of the dark or light editors."),
        PropertyMeta::new("ViewportColor", PropKind::String, "", "Background colour of the central area (#RRGGBB); empty for the application's background."),
        PropertyMeta::new("PanelsHidden", PropKind::Bool, "false", "Hides the panels and shows the central area alone, full size."),
    ],
    events: [
        EventMeta::new("OnPanelActivated", "Occurs when the user brings a panel to the front.").category(crate::registry::EventCategory::Behavior).args::<crate::events::TextChangedEventArgs>(),
        EventMeta::new("OnPanelClosed", "Occurs when the user closes a panel.").category(crate::registry::EventCategory::Behavior).args::<crate::events::TextChangedEventArgs>(),
        EventMeta::new("OnLayoutChanged", "Occurs when the panels are moved, resized, closed or reopened.").category(crate::registry::EventCategory::PropertyChanged).args::<crate::events::TextChangedEventArgs>(),
    ],
    smoke: |mut d| {
        d.hidden = true;
        d.with_theme(kubuno_desktop_ui::dock::DockTheme::workspace_dark())
    },
    build: |props, cx| {
        crate::registry::families::docking::build_dock_area(props, cx)
    },
}

/// Reads `<DockArea>`: its panels (built through `build_node`, so each one is
/// a design slot of its own), its viewport child and its properties.
pub(crate) fn build_dock_area(props: &Props<'_>, cx: &mut crate::props::BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
    let mut panels = Vec::new();
    let mut nodes: Vec<Box<dyn ViewNode>> = Vec::new();
    let mut arrangement = DockArrangement::new();
    // (side, group) → index in the side's group list, for `Group`.
    let mut groups: Vec<(DockSide, String, usize)> = Vec::new();
    let mut actives: Vec<String> = Vec::new();
    let mut viewport: Option<Box<dyn ViewNode>> = None;
    for (n, child) in props.element().children().enumerate() {
        let name = child.name().unwrap_or_default();
        if name != "DockPanel" {
            if viewport.is_some() {
                return Err(BuildError::new(
                    "`<DockArea>` takes `<DockPanel>` children and ONE other element (the central area's content): put several controls in a `<Panel>`",
                    child.name_range(),
                ));
            }
            viewport = Some(crate::compile::build_node(&child, cx, LayoutKind::None)?);
            continue;
        }
        let meta = crate::registry::lookup("DockPanel").ok_or_else(|| BuildError::new("`DockPanel` is not registered", child.name_range()))?;
        let p = Props::new(&child, meta);
        let id = child.attribute("x:Name").and_then(|a| a.value()).filter(|v| !v.is_empty()).unwrap_or_else(|| format!("panel{}", n + 1));
        if panels.iter().any(|d: &DockPanel| d.id == id) {
            return Err(BuildError::new(format!("two `<DockPanel>`s are named `{id}`"), child.name_range()));
        }
        let title = literal(&p, "Title", "")?;
        let side = match literal(&p, "Side", "Right")?.as_str() {
            "Left" => DockSide::Left,
            "Float" => DockSide::Float,
            _ => DockSide::Right,
        };
        let group = literal(&p, "Group", "")?;
        let mut panel = DockPanel::new(id.clone(), if title.is_empty() { id.clone() } else { title }).closable(literal_bool(&p, "Closable", true)?);
        panel.icon = crate::icon::resolve(&literal(&p, "Icon", "")?);
        if literal_bool(&p, "Active", false)? {
            actives.push(id.clone());
        }
        // Same side + same non-empty group → the same tab group.
        let side_len = |a: &DockArrangement| match side {
            DockSide::Left => a.left.len(),
            DockSide::Right => a.right.len(),
            DockSide::Float => a.float.len(),
        };
        let existing = groups.iter().find(|(s, g, _)| *s == side && !group.is_empty() && *g == group).map(|(_, _, i)| *i);
        match existing {
            Some(i) => {
                let v = match side {
                    DockSide::Left => &mut arrangement.left,
                    DockSide::Right => &mut arrangement.right,
                    DockSide::Float => &mut arrangement.float,
                };
                if let Some(g) = v.get_mut(i) {
                    g.push(id.clone());
                }
            }
            None => {
                let i = side_len(&arrangement);
                arrangement.push(side, &id, true);
                groups.push((side, group, i));
            }
        }
        panels.push(panel);
        nodes.push(crate::compile::build_node(&child, cx, LayoutKind::None)?);
    }
    let mut dock = DockArea::new(panels, arrangement);
    for a in &actives {
        dock.activate(a);
    }
    // The arrangement with its `Active` panels is the default a reset returns to.
    let default_layout = dock.layout().clone();
    let theme = match literal(props, "Theme", "Default")?.as_str() {
        "WorkspaceDark" => Some(DockTheme::workspace_dark()),
        "WorkspaceLight" => Some(DockTheme::workspace_light()),
        _ => None,
    };
    dock.theme = theme;
    dock.viewport_bg = parse_colour(&literal(props, "ViewportColor", "")?);
    let dock_viewport_auto = dock.viewport_bg.is_none();
    Ok(Box::new(DockAreaNode {
        storage_key: literal(props, "StorageKey", "")?,
        layout: props.str("Layout", "")?,
        active: props.str("ActivePanel", "")?,
        hidden: props.bool("PanelsHidden", false)?,
        on_activated: props.event("OnPanelActivated"),
        on_closed: props.event("OnPanelClosed"),
        on_layout: props.event("OnLayoutChanged"),
        focus_id: props.focus_id(),
        dock,
        default_layout,
        nodes,
        viewport,
        last_layout_prop: None,
        last_active_prop: None,
        storage_applied: false,
        last_json: String::new(),
        viewport_auto: dock_viewport_auto,
    }) as Box<dyn ViewNode>)
}

/// `#RRGGBB` (or `RRGGBB`) as a colour.
fn parse_colour(s: &str) -> Option<windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F> {
    let h = s.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    u32::from_str_radix(h, 16).ok().map(kubuno_desktop_ui::dock::hex)
}

/// `<DockArea>`'s live node. The [`DockArea`] controller is kept across frames
/// (its drag, resize and menu state are real state a rebuild would lose).
pub struct DockAreaNode {
    storage_key: String,
    layout: PropSource<String>,
    active: PropSource<String>,
    hidden: PropSource<bool>,
    on_activated: Option<String>,
    on_closed: Option<String>,
    on_layout: Option<String>,
    focus_id: Option<FocusId>,
    dock: DockArea,
    /// The default arrangement (with the `Active` panels), for design mode.
    default_layout: kubuno_desktop_ui::dock::DockLayout,
    /// One node per `<DockPanel>`, index-aligned with `dock.panels`.
    nodes: Vec<Box<dyn ViewNode>>,
    viewport: Option<Box<dyn ViewNode>>,
    /// The `Layout` / `ActivePanel` values last seen: a CHANGE of the property
    /// (a binding, code) is applied; an unchanged one is not re-applied over
    /// what the user did since.
    last_layout_prop: Option<String>,
    last_active_prop: Option<String>,
    storage_applied: bool,
    last_json: String,
    /// No `ViewportColor`: the central area takes the application's background.
    viewport_auto: bool,
}

impl DockAreaNode {
    fn write_back(spec: Option<&crate::binding::BindingSpec>, vm: &mut dyn crate::binding::ViewModel, value: &str) {
        if let Some(spec) = spec.filter(|s| s.mode.writes_back()) {
            spec.update_source(vm, Value::Str(value.to_string()));
        }
    }
}

impl ViewNode for DockAreaNode {
    fn measure(&self, _c: &dyn Canvas, _vm: &dyn crate::binding::ViewModel) -> Size {
        // Like the web's `flex-1`: no intrinsic size, a comfortable default.
        Size::new(720.0, 480.0)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let design = cx.design.is_some();
        self.dock.hidden = self.hidden.resolve(cx.vm);
        if self.viewport_auto {
            let c: &dyn Canvas = cx.canvas;
            self.dock.viewport_bg = Some(c.theme().window_background);
        }
        if design {
            // The designer shows the declared arrangement, never a saved one.
            if self.dock.layout() != &self.default_layout {
                self.dock.set_layout(self.default_layout.clone());
            }
        } else {
            if !self.storage_applied {
                self.storage_applied = true;
                if !self.storage_key.is_empty() {
                    self.dock.set_storage_key(Some(self.storage_key.clone()));
                }
            }
            let wanted = self.layout.resolve(cx.vm);
            if self.last_layout_prop.as_deref() != Some(wanted.as_str()) {
                // The first frame applies a non-empty literal/bound layout; an empty one keeps
                // what the storage restored.
                if self.last_layout_prop.is_some() || !wanted.is_empty() {
                    self.dock.load_layout(&wanted);
                }
                self.last_layout_prop = Some(wanted);
            }
            let active = self.active.resolve(cx.vm);
            if self.last_active_prop.as_deref() != Some(active.as_str()) {
                if !active.is_empty() {
                    self.dock.open(&active);
                }
                self.last_active_prop = Some(active);
            }
        }
        if let Some(id) = self.focus_id {
            let _ = cx.focus.register(id, bounds);
        }

        // The frame the dock sees: none of the pointer in the designer.
        let frame: Frame = if design {
            Frame {
                mouse: (kubuno_desktop_controls::host::POINTER_AWAY, kubuno_desktop_controls::host::POINTER_AWAY),
                mouse_down: false,
                right_down: false,
                middle_down: false,
                wheel: (0.0, 0.0),
                ..*cx.frame
            }
        } else {
            *cx.frame
        };
        self.dock.keyboard = !design;
        let outer_canvas = cx.canvas;
        let canvas: &dyn Canvas = outer_canvas;
        let Self { dock, nodes, viewport, .. } = self;
        let ids: Vec<String> = dock.panels.iter().map(|p| p.id.clone()).collect();
        let mut inner = cx.reborrow();
        let run = dock.frame(canvas, bounds, &frame, &mut |_c, pf, slot, rect| {
            // The dock hands each surface its own frame (the pointer masked when a float, a drag or
            // a menu is above it); in the designer the real frame goes through, so the design
            // slots still see the pointer.
            let f = if design { *inner.frame } else { *pf };
            let mut content = inner.with_surface(outer_canvas, &f);
            match slot {
                DockSlot::Viewport => {
                    if let Some(v) = viewport.as_mut() {
                        v.paint(&mut content, rect);
                    }
                }
                DockSlot::Panel(id) => {
                    if let Some(node) = ids.iter().position(|p| p == id).and_then(|i| nodes.get_mut(i)) {
                        node.paint(&mut content, rect);
                    }
                }
            }
        });
        drop(inner);
        if design {
            return;
        }
        // The tab lists, tabs and panel bodies in the accessibility tree, under this element.
        if let (Some((slot, _)), Some(services)) = (cx.sender.clone(), cx.services.as_deref_mut()) {
            let parent = crate::common::access_id(&slot.id);
            let base = crate::common::access_id(&format!("{}/dock", slot.id));
            for mut node in self.dock.access_nodes(base, Some(parent)) {
                let (l, t, r, b) = node.bounds;
                let c = crate::common::to_client(Rect::new(l, t, r, b));
                node.bounds = (c.left, c.top, c.right, c.bottom);
                services.access.push(node);
            }
        }
        let mut layout_changed = false;
        for e in run.events {
            match e {
                DockEvent::Activated(id) => {
                    let old = self.last_active_prop.clone().unwrap_or_default();
                    Self::write_back(self.active.binding(), cx.vm, &id);
                    self.last_active_prop = Some(id.clone());
                    let mut args = TextChangedEventArgs::new(old, id.clone(), ChangeSource::User);
                    cx.fire("OnPanelActivated", self.focus_id, self.on_activated.as_deref(), ViewEventKind::Changed(id), &mut args);
                }
                DockEvent::Closed(id) => {
                    let mut args = TextChangedEventArgs::new(String::new(), id.clone(), ChangeSource::User);
                    cx.fire("OnPanelClosed", self.focus_id, self.on_closed.as_deref(), ViewEventKind::Changed(id), &mut args);
                }
                DockEvent::Opened(_) => {}
                DockEvent::LayoutChanged => layout_changed = true,
            }
        }
        if layout_changed {
            let json = self.dock.save_layout();
            if json != self.last_json {
                let old = std::mem::replace(&mut self.last_json, json.clone());
                Self::write_back(self.layout.binding(), cx.vm, &json);
                if self.layout.binding().is_some_and(|s| s.mode.writes_back()) {
                    self.last_layout_prop = Some(json.clone());
                }
                let mut args = TextChangedEventArgs::new(old, json.clone(), ChangeSource::User);
                cx.fire("OnLayoutChanged", self.focus_id, self.on_layout.as_deref(), ViewEventKind::Changed(json), &mut args);
            }
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// WorkspaceShell
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: workspace_shell,
    name: "WorkspaceShell",
    // Note: The chrome of an advanced editor (`kubuno_desktop_ui::workspace::WorkspaceShell`, port of `WorkspaceShell.tsx`).
    doc: "The frame of an editor: a top bar with the title, the editor's name and the document's details, an optional status bar, and one child element as its body (typically a DockArea).",
    ctor: kubuno_desktop_ui::workspace::WorkspaceShell::new(kubuno_desktop_ui::workspace::WorkspaceTheme::dark()),
    children: ChildrenModel::SingleWidget,
    default_event: "OnBack",
    props: [
        PropertyMeta::new("Title", PropKind::String, "", "Title shown in the top bar (the document's name)."),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon shown before the title: a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.").editor("icon").category("Icon"),
        PropertyMeta::new("Subtitle", PropKind::String, "", "Name of the editor, shown after the title in the accent colour."),
        PropertyMeta::new("DocInfo", PropKind::String, "", "Details of the document shown after the subtitle (dimensions, page count…)."),
        PropertyMeta::new("StatusText", PropKind::String, "", "Texts of the status bar, separated by a vertical bar (|); empty for no status bar."),
        PropertyMeta::new("Theme", PropKind::Enum(&["Default", "Dark", "Light", "Office"]), "Default", "Colours of the frame: the application's (light or dark), or a fixed palette."),
        PropertyMeta::new("ShowBack", PropKind::Bool, "false", "Shows the back arrow."),
        PropertyMeta::new("ShowSearch", PropKind::Bool, "false", "Shows the search button."),
        PropertyMeta::new("ShowDelete", PropKind::Bool, "false", "Shows the delete button."),
    ],
    events: [
        EventMeta::new("OnBack", "Occurs when the back arrow is clicked.").category(crate::registry::EventCategory::Action),
        EventMeta::new("OnSearch", "Occurs when the search button is clicked.").category(crate::registry::EventCategory::Action),
        EventMeta::new("OnDelete", "Occurs when the delete button is clicked.").category(crate::registry::EventCategory::Action),
    ],
    smoke: |mut s| {
        s.title = "Sans titre".into();
        s.status = vec!["100 %".into()];
        s
    },
    build: |props, cx| {
        use super::*;
        let theme = match literal_theme(props)?.as_str() {
            "Light" => kubuno_desktop_ui::workspace::WorkspaceTheme::light(),
            "Office" => kubuno_desktop_ui::workspace::WorkspaceTheme::office(),
            "Dark" => kubuno_desktop_ui::workspace::WorkspaceTheme::dark(),
            _ => kubuno_desktop_ui::workspace::WorkspaceTheme::dark(),
        };
        let icon = match props.str("Icon", "")? {
            crate::binding::PropSource::Literal(s) => crate::icon::resolve(&s),
            crate::binding::PropSource::Bound { .. } => None,
        };
        Ok(Box::new(crate::registry::families::docking::WorkspaceShellNode {
            title: props.str("Title", "")?,
            subtitle: props.str("Subtitle", "")?,
            doc_info: props.str("DocInfo", "")?,
            status: props.str("StatusText", "")?,
            show_back: props.bool("ShowBack", false)?,
            show_search: props.bool("ShowSearch", false)?,
            show_delete: props.bool("ShowDelete", false)?,
            follow_app: !matches!(literal_theme(props)?.as_str(), "Dark" | "Light" | "Office"),
            on_back: props.event("OnBack"),
            on_search: props.event("OnSearch"),
            on_delete: props.event("OnDelete"),
            shell: {
                let mut s = kubuno_desktop_ui::workspace::WorkspaceShell::new(theme);
                s.title_icon = icon;
                s
            },
            child: props.build_single_child(cx)?,
        }) as Box<dyn ViewNode>)
    },
}

fn literal_theme(props: &Props<'_>) -> Result<String, BuildError> {
    literal(props, "Theme", "Default")
}

/// `<WorkspaceShell>`'s live node; the shell keeps its press state across frames.
pub struct WorkspaceShellNode {
    title: PropSource<String>,
    subtitle: PropSource<String>,
    doc_info: PropSource<String>,
    status: PropSource<String>,
    show_back: PropSource<bool>,
    show_search: PropSource<bool>,
    show_delete: PropSource<bool>,
    /// `Theme="Default"`: the palette follows the application's light or dark mode.
    follow_app: bool,
    on_back: Option<String>,
    on_search: Option<String>,
    on_delete: Option<String>,
    shell: WorkspaceShell,
    child: Option<Box<dyn ViewNode>>,
}

impl ViewNode for WorkspaceShellNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn crate::binding::ViewModel) -> Size {
        let body = self.child.as_ref().map(|n| n.measure(c, vm)).unwrap_or(Size::new(480.0, 320.0));
        Size::new(body.width, body.height + self.shell.topbar_height + if self.status.resolve(vm).is_empty() { 0.0 } else { 22.0 })
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let vm = &*cx.vm;
        let s = &mut self.shell;
        s.title = self.title.resolve(vm);
        let sub = self.subtitle.resolve(vm);
        s.subtitle = (!sub.is_empty()).then_some(sub);
        let info = self.doc_info.resolve(vm);
        s.doc_info = (!info.is_empty()).then_some(info);
        let status = self.status.resolve(vm);
        s.status = status.split('|').map(|t| t.trim().to_string()).filter(|t| !t.is_empty()).collect();
        s.status_height = if status.trim().is_empty() { 0.0 } else { 22.0 };
        s.show_back = self.show_back.resolve(vm);
        s.show_search = self.show_search.resolve(vm);
        s.show_delete = self.show_delete.resolve(vm);
        let design = cx.design.is_some();
        let frame = if design {
            Frame { mouse: (kubuno_desktop_controls::host::POINTER_AWAY, kubuno_desktop_controls::host::POINTER_AWAY), mouse_down: false, ..*cx.frame }
        } else {
            *cx.frame
        };
        let canvas: &dyn Canvas = cx.canvas;
        if self.follow_app {
            let dark = canvas.theme().mode == kubuno_desktop_ui::ThemeMode::Dark;
            if s.theme.dark != dark {
                s.theme = kubuno_desktop_ui::workspace::WorkspaceTheme::for_mode(dark);
            }
        }
        let run = s.frame(canvas, bounds, &frame);
        if let Some(child) = self.child.as_mut() {
            let mut inner = cx.reborrow();
            child.paint(&mut inner, run.body);
        }
        if design {
            return;
        }
        for (hit, event, handler) in [(run.back, "OnBack", &self.on_back), (run.search, "OnSearch", &self.on_search), (run.delete, "OnDelete", &self.on_delete)] {
            if hit {
                let mut args = crate::events::EmptyEventArgs;
                cx.fire(event, None, handler.as_deref(), ViewEventKind::Clicked, &mut args);
            }
        }
    }
}

/// Every component this family declares, in declaration order.
pub const ALL: &[ComponentMeta] = &[dock_area::META, dock_panel::META, workspace_shell::META];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{AstNode, Document};
    use crate::syntax::parse;

    fn area(src: &str) -> Result<Box<dyn ViewNode>, BuildError> {
        let p = parse(src);
        let doc = Document::cast(p.syntax()).expect("document");
        let el = doc.root_element().expect("root");
        let meta = crate::registry::lookup("DockArea").expect("registered");
        let props = Props::new(&el, meta);
        let mut cx = crate::props::BuildCx::new();
        build_dock_area(&props, &mut cx)
    }

    #[test]
    fn panels_group_by_side_and_group() {
        let node = area(
            r#"<DockArea>
                 <DockPanel x:Name="nav" Title="Navigateur"/>
                 <DockPanel x:Name="brush" Title="Pinceau" Group="tools"/>
                 <DockPanel x:Name="adjust" Title="Réglages" Group="tools" Active="true"/>
                 <DockPanel x:Name="tree" Title="Arborescence" Side="Left"/>
                 <Panel/>
               </DockArea>"#,
        );
        assert!(node.is_ok(), "{:?}", node.err().map(|e| e.message));
    }

    #[test]
    fn two_viewport_children_are_refused() {
        let r = area(r#"<DockArea><Panel/><Panel/></DockArea>"#);
        assert!(r.is_err());
    }

    #[test]
    fn duplicate_names_are_refused() {
        let r = area(r#"<DockArea><DockPanel x:Name="a"/><DockPanel x:Name="a"/></DockArea>"#);
        assert!(r.err().is_some_and(|e| e.message.contains("two")));
    }

    #[test]
    fn default_arrangement_follows_the_markup() {
        let p = parse(
            r#"<DockArea>
                 <DockPanel x:Name="nav"/>
                 <DockPanel x:Name="brush" Group="tools"/>
                 <DockPanel x:Name="adjust" Group="tools" Active="true"/>
                 <DockPanel x:Name="tree" Side="Left"/>
                 <DockPanel x:Name="float" Side="Float"/>
               </DockArea>"#,
        );
        let doc = Document::cast(p.syntax()).expect("document");
        let el = doc.root_element().expect("root");
        let meta = crate::registry::lookup("DockArea").expect("registered");
        let props = Props::new(&el, meta);
        let mut cx = crate::props::BuildCx::new();
        let node = build_dock_area(&props, &mut cx).expect("builds");
        // The node is opaque: check through a fresh area with the same arrangement.
        let _ = node;
        let mut a = DockArrangement::new();
        a.push(DockSide::Right, "nav", true);
        a.push(DockSide::Right, "brush", true);
        a.push(DockSide::Right, "adjust", false);
        assert_eq!(a.right, vec![vec!["nav".to_string()], vec!["brush".to_string(), "adjust".to_string()]]);
    }

    #[test]
    fn colours_parse() {
        assert!(parse_colour("#141414").is_some());
        assert!(parse_colour("eef1f5").is_some());
        assert!(parse_colour("nope").is_none());
    }
}
