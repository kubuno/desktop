//! The window's accessibility tree, exposed to UI Automation (Narrator, NVDA, Accessibility
//! Insights) through AccessKit — `vskubuno/docs/EVENTS.md` §16, `AccessibleName`/`Description`/
//! `Role`.
//!
//! The controls are drawn, not windowed: Windows cannot see them. A page therefore describes them
//! each frame with [`publish`] (one [`AccessNode`] per element, with its role, name, bounds and
//! state), and the host answers `WM_GETOBJECT` from that description through
//! `accesskit_windows::Adapter`. Nothing is built until an assistive technology asks (AccessKit is
//! lazy): a page that publishes pays for a `Vec` per frame, no more. What the assistive technology
//! asks for (press a button, move the focus) comes back through [`take_actions`] and wakes the
//! window.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use accesskit::{Action, ActionHandler, ActionRequest, ActivationHandler, Node, NodeId, Role, Toggled, TreeId, TreeInfo, TreeUpdate};

/// What an element is, for assistive technology.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AccessRole {
    #[default]
    Group,
    Pane,
    Button,
    CheckBox,
    RadioButton,
    Switch,
    TextInput,
    /// A password field: UI Automation reports `IsPassword`, and its value is never the secret.
    PasswordInput,
    MultilineTextInput,
    Label,
    Link,
    List,
    ListItem,
    ComboBox,
    Slider,
    ProgressIndicator,
    SpinButton,
    Tab,
    TabList,
    TabPanel,
    Table,
    Row,
    Cell,
    ColumnHeader,
    Tree,
    TreeItem,
    Toolbar,
    StatusBar,
    Image,
    Separator,
    Alert,
    Dialog,
    Menu,
    MenuItem,
    MenuBar,
    ScrollBar,
    Tooltip,
    Document,
    Window,
    Unknown,
}

impl AccessRole {
    fn accesskit(self) -> Role {
        match self {
            AccessRole::Group => Role::Group,
            AccessRole::Pane => Role::Pane,
            AccessRole::Button => Role::Button,
            AccessRole::CheckBox => Role::CheckBox,
            AccessRole::RadioButton => Role::RadioButton,
            AccessRole::Switch => Role::Switch,
            AccessRole::TextInput => Role::TextInput,
            AccessRole::PasswordInput => Role::PasswordInput,
            AccessRole::MultilineTextInput => Role::MultilineTextInput,
            AccessRole::Label => Role::Label,
            AccessRole::Link => Role::Link,
            AccessRole::List => Role::List,
            AccessRole::ListItem => Role::ListItem,
            AccessRole::ComboBox => Role::ComboBox,
            AccessRole::Slider => Role::Slider,
            AccessRole::ProgressIndicator => Role::ProgressIndicator,
            AccessRole::SpinButton => Role::SpinButton,
            AccessRole::Tab => Role::Tab,
            AccessRole::TabList => Role::TabList,
            AccessRole::TabPanel => Role::TabPanel,
            AccessRole::Table => Role::Table,
            AccessRole::Row => Role::Row,
            AccessRole::Cell => Role::Cell,
            AccessRole::ColumnHeader => Role::ColumnHeader,
            AccessRole::Tree => Role::Tree,
            AccessRole::TreeItem => Role::TreeItem,
            AccessRole::Toolbar => Role::Toolbar,
            AccessRole::StatusBar => Role::Status,
            AccessRole::Image => Role::Image,
            AccessRole::Separator => Role::Splitter,
            AccessRole::Alert => Role::Alert,
            AccessRole::Dialog => Role::Dialog,
            AccessRole::Menu => Role::Menu,
            AccessRole::MenuItem => Role::MenuItem,
            AccessRole::MenuBar => Role::MenuBar,
            AccessRole::ScrollBar => Role::ScrollBar,
            AccessRole::Tooltip => Role::Tooltip,
            AccessRole::Document => Role::Document,
            AccessRole::Window => Role::Window,
            AccessRole::Unknown => Role::Unknown,
        }
    }
}

/// One element of the page, as assistive technology sees it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccessNode {
    /// Unique among the page's nodes, stable across frames, never 0 (the window's).
    pub id: u64,
    /// The element containing it, `None` for a top-level element of the page.
    pub parent: Option<u64>,
    pub role: AccessRole,
    /// What a screen reader announces.
    pub name: String,
    pub description: String,
    /// A field's text, a slider's value…
    pub value: Option<String>,
    /// Where it is painted, in client DIP.
    /// `(left, top, right, bottom)`.
    pub bounds: (f32, f32, f32, f32),
    pub focusable: bool,
    pub disabled: bool,
    /// A check box, a switch, a radio button: its state.
    pub checked: Option<bool>,
    /// It can be pressed (a button, a link, a check box).
    pub clickable: bool,
    pub read_only: bool,
    /// Its keyboard shortcut, as announced ("Alt+S").
    pub access_key: Option<String>,
    /// A menu item, a combo box, a tree item: whether what it opens is open (UI Automation's
    /// ExpandCollapse pattern). `None`: it opens nothing.
    pub expanded: Option<bool>,
}

/// What assistive technology asks of an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessAction {
    /// Press it (a button's click).
    Click,
    /// Give it the keyboard focus.
    Focus,
}

/// A page's description of this frame's elements (see the module doc).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccessTree {
    pub nodes: Vec<AccessNode>,
    /// The element holding the keyboard focus.
    pub focus: Option<u64>,
    /// The window's name (its title).
    pub title: String,
    /// DIP → physical pixels, for the bounds.
    pub scale: f32,
}

thread_local! {
    /// The last tree a page published, for the host to push after the frame.
    static PUBLISHED: RefCell<Option<AccessTree>> = const { RefCell::new(None) };
}

/// Actions asked by assistive technology, from any thread, taken by the page of the window they
/// are for (several host windows may share a thread): `(window, node, action)`.
static ACTIONS: Mutex<Vec<(isize, u64, AccessAction)>> = Mutex::new(Vec::new());

/// Publishes this frame's elements (call it once per frame, after painting).
pub fn publish(tree: AccessTree) {
    PUBLISHED.with(|p| *p.borrow_mut() = Some(tree));
}

/// The actions assistive technology asked for since the last call, oldest first — those of the
/// window whose frame is running.
pub fn take_actions() -> Vec<(u64, AccessAction)> {
    let window = crate::host::input::main_hwnd().map_or(0, |h| h.0 as isize);
    let mut all = match ACTIONS.lock() {
        Ok(a) => a,
        Err(poisoned) => poisoned.into_inner(),
    };
    let (mine, others): (Vec<_>, Vec<_>) = std::mem::take(&mut *all).into_iter().partition(|(w, _, _)| *w == window);
    *all = others;
    mine.into_iter().map(|(_, node, action)| (node, action)).collect()
}

/// The published tree, taken by the host after the frame.
pub(crate) fn take_published() -> Option<AccessTree> {
    PUBLISHED.with(|p| p.try_borrow_mut().ok().and_then(|mut t| t.take()))
}

/// The tree published by the frame that ran last on this thread, if the host did not take it yet (tests and
/// diagnostics: a page's accessibility tree without a window).
pub fn last_published() -> Option<AccessTree> {
    PUBLISHED.with(|p| p.try_borrow().ok().and_then(|t| t.clone()))
}

/// Puts back a tree taken with [`take_published`] (`super::window_tls`: per-window state).
pub(crate) fn put_published(tree: Option<AccessTree>) {
    PUBLISHED.with(|p| {
        if let Ok(mut slot) = p.try_borrow_mut() {
            *slot = tree;
        }
    });
}

const ROOT: NodeId = NodeId(0);

/// The AccessKit update describing `tree` (a full tree: every node, with the window as root).
pub fn tree_update(tree: &AccessTree) -> TreeUpdate {
    let scale = if tree.scale > 0.0 { f64::from(tree.scale) } else { 1.0 };
    let mut root = Node::new(Role::Window);
    if !tree.title.is_empty() {
        root.set_label(tree.title.clone());
    }
    // A node id met twice (a page that reused an element's id) would make AccessKit refuse the whole tree (it
    // panics on a duplicate child): the first node of an id is kept, the others are left out, and a node is never
    // its own parent.
    let mut seen = std::collections::HashSet::new();
    let unique: Vec<&AccessNode> = tree.nodes.iter().filter(|n| n.id != ROOT.0 && seen.insert(n.id)).collect();
    if unique.len() != tree.nodes.len() {
        tracing::warn!("accessibility tree: {} node(s) left out (duplicate or reserved ids)", tree.nodes.len() - unique.len());
    }
    let ids: std::collections::HashSet<u64> = unique.iter().map(|n| n.id).collect();
    let mut children: std::collections::HashMap<u64, Vec<NodeId>> = std::collections::HashMap::new();
    let mut top = Vec::new();
    for n in &unique {
        match n.parent.filter(|p| ids.contains(p) && *p != n.id) {
            Some(p) => children.entry(p).or_default().push(NodeId(n.id)),
            None => top.push(NodeId(n.id)),
        }
    }
    root.set_children(top);
    let mut nodes = vec![(ROOT, root)];
    for n in &unique {
        let mut node = Node::new(n.role.accesskit());
        if !n.name.is_empty() {
            node.set_label(n.name.clone());
        }
        if !n.description.is_empty() {
            node.set_description(n.description.clone());
        }
        if let Some(v) = &n.value {
            node.set_value(v.clone());
        } else if n.role == AccessRole::Label && !n.name.is_empty() {
            // A static text is announced through its value (AccessKit's text roles): its words are
            // its name.
            node.set_value(n.name.clone());
        }
        if let Some(key) = &n.access_key {
            node.set_access_key(key.clone());
        }
        node.set_bounds(accesskit::Rect {
            x0: f64::from(n.bounds.0) * scale,
            y0: f64::from(n.bounds.1) * scale,
            x1: f64::from(n.bounds.2) * scale,
            y1: f64::from(n.bounds.3) * scale,
        });
        if n.focusable && !n.disabled {
            node.add_action(Action::Focus);
        }
        if n.clickable && !n.disabled {
            node.add_action(Action::Click);
        }
        if n.disabled {
            node.set_disabled();
        }
        if n.read_only {
            node.set_read_only();
        }
        if let Some(checked) = n.checked {
            node.set_toggled(Toggled::from(checked));
        }
        if let Some(expanded) = n.expanded {
            node.set_expanded(expanded);
            node.add_action(if expanded { Action::Collapse } else { Action::Expand });
        }
        if let Some(kids) = children.remove(&n.id) {
            node.set_children(kids);
        }
        nodes.push((NodeId(n.id), node));
    }
    let focus = tree.focus.filter(|f| ids.contains(f)).map(NodeId).unwrap_or(ROOT);
    TreeUpdate { nodes, tree: Some(TreeInfo::new(ROOT)), tree_id: TreeId::ROOT, focus }
}

/// The window's AccessKit adapter and the last tree, owned by the host.
pub(crate) struct Access {
    adapter: accesskit_windows::Adapter,
    last: Arc<Mutex<Option<TreeUpdate>>>,
}

/// Answers AccessKit's lazy activation with the last tree.
struct Activation(Arc<Mutex<Option<TreeUpdate>>>);

impl ActivationHandler for Activation {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0.lock().ok().and_then(|t| t.clone())
    }
}

/// Queues what assistive technology asks for and wakes the window.
struct Actions(crate::host::UiWaker, isize);

impl ActionHandler for Actions {
    fn do_action(&mut self, request: ActionRequest) {
        let action = match request.action {
            // Expanding or collapsing a menu item opens or closes its menu: a click.
            Action::Click | Action::Expand | Action::Collapse => AccessAction::Click,
            Action::Focus => AccessAction::Focus,
            _ => return,
        };
        match ACTIONS.lock() {
            Ok(mut a) => a.push((self.1, request.target_node.0, action)),
            Err(poisoned) => poisoned.into_inner().push((self.1, request.target_node.0, action)),
        }
        self.0.wake();
    }
}

impl Access {
    /// The adapter of the window `hwnd` (not while handling `WM_GETOBJECT`).
    pub(crate) fn new(hwnd: windows::Win32::Foundation::HWND, focused: bool) -> Self {
        let last = Arc::new(Mutex::new(None));
        let adapter = accesskit_windows::Adapter::new(hwnd, focused, Actions(crate::host::ui_waker(), hwnd.0 as isize));
        Self { adapter, last }
    }

    /// `WM_GETOBJECT`: `Some(answer)` when AccessKit answers it. The answer is called once the
    /// caller no longer borrows the adapter: returning the provider to UI Automation may send a
    /// nested `WM_GETOBJECT`.
    pub(crate) fn get_object(
        &mut self,
        wparam: windows::Win32::Foundation::WPARAM,
        lparam: windows::Win32::Foundation::LPARAM,
    ) -> Option<Box<dyn FnOnce() -> windows::Win32::Foundation::LRESULT>> {
        let mut activation = Activation(self.last.clone());
        // A panic of AccessKit (a tree it refuses) must neither end the window nor silence it for good: it is logged
        // and UI Automation gets the default answer this time.
        let adapter = &mut self.adapter;
        let answer = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| adapter.handle_wm_getobject(wparam, lparam, &mut activation))) {
            Ok(answer) => answer?,
            Err(_) => {
                tracing::error!("accessibility: AccessKit failed to answer WM_GETOBJECT (see the panic above); the window answers as a plain window");
                return None;
            }
        };
        Some(Box::new(move || answer.into()))
    }

    /// Pushes a newly published tree (raising the resulting events).
    pub(crate) fn update(&mut self, tree: &AccessTree) {
        let update = tree_update(tree);
        if let Ok(mut last) = self.last.lock() {
            if last.as_ref() == Some(&update) {
                return;
            }
            *last = Some(update.clone());
        }
        let adapter = &mut self.adapter;
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| adapter.update_if_active(|| update).map(|events| events.raise()))) {
            Ok(_) => {}
            Err(_) => tracing::error!("accessibility: AccessKit refused a tree update (see the panic above); the next frame publishes again"),
        }
    }

    /// The window gained or lost the keyboard focus.
    pub(crate) fn focus_changed(&mut self, focused: bool) {
        if let Some(events) = self.adapter.update_window_focus_state(focused) {
            events.raise();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Found with user controls: a page that reused element ids made AccessKit panic on a duplicate child (UI
    /// Automation then saw no element at all). The duplicates are left out of the update instead.
    #[test]
    fn duplicate_node_ids_are_left_out_of_the_update() {
        let tree = AccessTree {
            nodes: vec![
                AccessNode { id: 7, role: AccessRole::Pane, ..Default::default() },
                AccessNode { id: 8, parent: Some(7), role: AccessRole::Button, ..Default::default() },
                AccessNode { id: 8, parent: Some(7), role: AccessRole::Button, ..Default::default() },
                AccessNode { id: 9, parent: Some(9), role: AccessRole::Button, ..Default::default() },
            ],
            ..Default::default()
        };
        let update = tree_update(&tree);
        let ids: Vec<u64> = update.nodes.iter().map(|(id, _)| id.0).collect();
        assert_eq!(ids, [0, 7, 8, 9]);
        let pane = &update.nodes[1].1;
        assert_eq!(pane.children(), [NodeId(8)], "one child 8");
        assert_eq!(update.nodes[0].1.children(), [NodeId(7), NodeId(9)], "a node that names itself as its parent is top-level");
    }

    #[test]
    fn a_published_tree_becomes_a_full_accesskit_tree_under_the_window() {
        let tree = AccessTree {
            nodes: vec![
                AccessNode { id: 7, role: AccessRole::Pane, name: "Main".into(), bounds: (0.0, 0.0, 100.0, 50.0), ..Default::default() },
                AccessNode {
                    id: 9,
                    parent: Some(7),
                    role: AccessRole::Button,
                    name: "Save".into(),
                    description: "Saves the file".into(),
                    bounds: (10.0, 10.0, 60.0, 30.0),
                    focusable: true,
                    clickable: true,
                    access_key: Some("Alt+S".into()),
                    ..Default::default()
                },
                AccessNode { id: 11, parent: Some(7), role: AccessRole::CheckBox, checked: Some(true), disabled: true, ..Default::default() },
            ],
            focus: Some(9),
            title: "Demo".into(),
            scale: 1.5,
        };
        let update = tree_update(&tree);
        assert_eq!(update.nodes.len(), 4);
        assert_eq!(update.focus, NodeId(9));
        let (_, root) = &update.nodes[0];
        assert_eq!(root.role(), Role::Window);
        assert_eq!(root.children(), &[NodeId(7)]);
        let (_, pane) = update.nodes.iter().find(|(id, _)| *id == NodeId(7)).unwrap();
        assert_eq!(pane.children(), &[NodeId(9), NodeId(11)]);
        let (_, save) = update.nodes.iter().find(|(id, _)| *id == NodeId(9)).unwrap();
        assert_eq!(save.label(), Some("Save"));
        assert_eq!(save.description(), Some("Saves the file"));
        assert!(save.supports_action(Action::Click) && save.supports_action(Action::Focus));
        assert_eq!(save.bounds().map(|b| (b.x0, b.y1)), Some((15.0, 45.0)));
        let (_, check) = update.nodes.iter().find(|(id, _)| *id == NodeId(11)).unwrap();
        assert_eq!(check.toggled(), Some(Toggled::True));
        assert!(check.is_disabled() && !check.supports_action(Action::Click));
    }
}
