//! `TreeView` and `ListView` — the two "view" controls, with their item models.
//!
//! ## Why these two share a file
//!
//! In WinForms both descend straight from `Control` (neither has an intermediate
//! base), yet they are the two controls whose *content* is a real data structure
//! rather than a single caption: a `TreeView` owns a tree of `TreeNode`, a
//! `ListView` owns rows (`ListViewItem` + `ListViewItem.ListViewSubItem`),
//! `Columns` (`ColumnHeader`) and `Groups` (`ListViewGroup`). Porting them well
//! means porting those models as *pure, testable values* — which is the bulk of
//! this module — and only then painting them.
//!
//! ## What is deferred, and why
//!
//! Several declared properties reference toolkit resources that have **no port
//! primitive**, exactly as `ControlBase` already omits `BackgroundImage` (an
//! `Image`). Rather than invent a type, they are documented here and on the
//! owning struct, never silently treated as something else:
//!
//! * **`ImageList` / `StateImageList` / `LargeImageList` / `SmallImageList` /
//!   `GroupImageList`** — an `ImageList` is a device-bound bitmap collection; a
//!   control paints images the host resolves for it (`Canvas::image`), so these
//!   collections are not modelled. The *indices* into them (`ImageIndex`,
//!   `SelectedImageIndex`, …) are primitives and are kept.
//! * **`TreeViewNodeSorter` / `ListViewItemSorter`** — an `IComparer` is a
//!   callback; a pure value cannot hold one. `Sorted`/`Sorting` are honoured by
//!   comparing node/item `Text`.
//! * **`DrawMode` / `OwnerDraw`** — owner-draw hands painting back to the host;
//!   this port always paints itself, so the flag is stored but not acted on.
//! * **`BackColor` / `ForeColor` / `Text` / `BackgroundImageLayout`** are
//!   carried by `ControlBase` (ambient) and reached through `Deref` — they are
//!   *not* re-declared here, per the library's "do not re-implement a base"
//!   rule, even though the catalogue lists them as re-declared on these types.
//!
//! ## How they are painted
//!
//! Both are **native common controls**, and both are repainted here as the
//! toolkit paints them, not as the Kubuno design system would: the ground is
//! `Window`, the item text `WindowText`, the selection the system's own
//! `Highlight` / `HighlightText`, and there is not a rounded corner anywhere.
//! Every colour comes from [`crate::system::Visuals`] — never a literal, never
//! `Canvas::theme()`.
//!
//! Two facts were read off the reference sheet (`08-views.png`, captured from
//! the real `System.Windows.Forms` on this machine) rather than assumed, and
//! both happen to be system colours *exactly*:
//!
//! * the `TreeView`'s dotted connecting lines are `#6D6D6D` — `GrayText`, not
//!   `ControlDark` (`#A0A0A0`);
//! * the `ListView`'s `GridLines` are `#F0F0F0` — `Control`, not `ControlDark`.
//!
//! ## The chrome that is not a system colour — asked of the theme
//!
//! Three things on the sheet are painted by the *visual-styles theme* rather
//! than by comctl32, and none of them is any entry of `SystemColors`: the field
//! border (a flat 1 px `#ABADB3`), the Details header (a `#FFFFFF` face with an
//! `#E5E5E5` rule per column), and the expand glyph (a `#919191` outline round a
//! pale gradient, with a `#4B63A7` sign). They used to be approximated with the
//! classic recipe and documented as the one place this family knowingly differed
//! from the sheet. They are now the genuine part, rendered by `uxtheme.dll`
//! through [`crate::theme`] — so they are identical **by construction** rather
//! than by a colour written down here.
//!
//! The classic recipe is still in the file, behind `if !c.draw_theme_part(…)`:
//! with visual styles off it is not a fallback but the only correct rendering,
//! and `KUBUNO_CONTROLS_CLASSIC=1` ([`crate::theme::CLASSIC_ENV`]) exercises it
//! on a themed machine.
//!
//! ## The two parts this family deliberately does **not** ask for
//!
//! `LVP_LISTITEM` and `TVP_TREEITEM` are the obvious names for a row's selection
//! band, and both are wrong here — not "a shade off", but not present at all:
//! `IsThemePartDefined` answers **false** for them in every state on this
//! machine's theme, because Windows Vista moved item rendering to the
//! `Explorer::` subclass and a `ListView`/`TreeView` only gets it after an
//! explicit `SetWindowTheme(hwnd, L"Explorer", …)`, which WinForms does not do.
//!
//! The trap is that asking anyway does not fail. `DrawThemeBackground` on an
//! undefined part returns `S_OK` and paints a generic `#FFFFFF` box inside a
//! `#828790` frame, so [`ControlCanvas::draw_theme_part`] reports `true` and a
//! family that trusted it would draw a white framed box over every row. And the
//! `Explorer::` parts, which *are* defined, render the modern rounded `#CCCCCC`
//! selection — also not the sheet, which shows the flat `Control` grey.
//!
//! So the bands stay the system colours the sheet actually shows, and the
//! `the_item_parts_are_not_in_this_theme` test pins the measurement: if a future
//! Windows defines them, that test fails and the decision is revisited rather
//! than quietly drifting.
//!
//! ## Focus
//!
//! Selection colours depend on focus in the real toolkit — blue when the control
//! has it, grey `Control` when it does not (and, when `HideSelection` is set,
//! nothing at all). The family models that through
//! [`Control::paint_with_state`]; plain [`Control::paint`] paints the **focused**
//! look, because a control that is asked to paint without a state has no way to
//! know otherwise. That is why the reference sheet's `ListView` (which is not
//! the focused control on the form) shows a grey selection where this port shows
//! blue.

use std::ops::{Deref, DerefMut};

use windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F;

use crate::containers;
use crate::control::{Control, ControlBase, ControlCanvas, ControlState};
use crate::enums::{BorderStyle, HorizontalAlignment, Size};
use crate::system::{edge_interior, Border3DSide, Border3DStyle, SystemColors};
use crate::theme;
use crate::theme::part::EP_EDITTEXT;
use crate::theme::state::{ETS_DISABLED, ETS_NORMAL};
use crate::{Canvas, Rect};

/// The `HEADER` and `TREEVIEW` part and state ids this family draws with.
///
/// Local to the file rather than added to [`crate::theme::part`] because the
/// eight other families are migrating in parallel and a shared table is the one
/// place they would all collide; the `EDIT` ids this family also needs already
/// live there and are imported above.
///
/// Re-exported from the Windows SDK's own constants rather than written as
/// numbers: a part id is exactly the kind of magic number that is copied wrong
/// once and then looks merely "a bit off" forever — and, worse here, an id that
/// names nothing still *draws* something (see the module docs).
mod part {
    use windows::Win32::UI::Controls as sdk;

    /// `HEADER` — one column button of the Details header band. It paints the
    /// `#FFFFFF` face **and** the `#E5E5E5` rule down its own right edge, which
    /// is the whole of what the sheet shows between two columns.
    ///
    /// **Not** `HP_HEADERITEMLEFT`/`HP_HEADERITEMRIGHT`, whose names suggest the
    /// first and last column: neither is defined in this theme, so both render
    /// the fallback — a plain white box with **no** rule at all.
    pub const HP_HEADERITEM: i32 = sdk::HP_HEADERITEM.0;
    /// `HEADER` — the sort triangle. A *TrueSize* part: it draws its own 9×5 at
    /// 96 DPI centred in whatever rectangle it is given, never stretched.
    pub const HP_HEADERSORTARROW: i32 = sdk::HP_HEADERSORTARROW.0;
    /// `TREEVIEW` — the expand/collapse glyph. Also TrueSize, and its natural
    /// side at 96 DPI is exactly [`super::GLYPH_BOX`], so the box this family
    /// already reserved is the box the theme wants.
    ///
    /// **Not** `TVP_HOTGLYPH`, which is undefined here and renders the fallback.
    pub const TVP_GLYPH: i32 = sdk::TVP_GLYPH.0;

    /// `HP_HEADERITEM` — at rest. The `#FFFFFF` face of the sheet.
    pub const HIS_NORMAL: i32 = sdk::HIS_NORMAL.0;
    // The painter always asks for `HIS_NORMAL`, so these two are reached only by
    // the test that pins their faces. That is deliberate and is not a gap to be
    // filled by wiring `ControlState` in: hotness is per-**column**, and the only
    // pointer state a control is handed is its own — using it would light every
    // header at once because the pointer is over a row far below. Measuring them
    // now means the day a `ColumnHeader` carries a hover flag (an item-model
    // change, which this pass may not make) the colours are already known and
    // already guarded against a theme update.
    /// `HP_HEADERITEM` — the pointer is over this column (`#D9EBF9`).
    #[allow(dead_code)]
    pub const HIS_HOT: i32 = sdk::HIS_HOT.0;
    /// `HP_HEADERITEM` — the pointer is down on this column (`#BCDCF4`).
    #[allow(dead_code)]
    pub const HIS_PRESSED: i32 = sdk::HIS_PRESSED.0;

    /// `HP_HEADERSORTARROW` — ascending, the triangle pointing up.
    pub const HSAS_SORTEDUP: i32 = sdk::HSAS_SORTEDUP.0;
    /// `HP_HEADERSORTARROW` — descending.
    pub const HSAS_SORTEDDOWN: i32 = sdk::HSAS_SORTEDDOWN.0;

    /// `TVP_GLYPH` — a collapsed node: the glyph carries a **plus**.
    pub const GLPS_CLOSED: i32 = sdk::GLPS_CLOSED.0;
    /// `TVP_GLYPH` — an expanded node: a minus.
    pub const GLPS_OPENED: i32 = sdk::GLPS_OPENED.0;
}

// ── Family-local enumerations ────────────────────────────────────────────────
//
// These live here, not in `enums.rs`, because they are declared by exactly one
// of these two controls and shared with no other family. `enums.rs` is reserved
// for the cross-cutting enums (`DockStyle`, `BorderStyle`…). Members and their
// discriminants come from the reflection catalogue, and the default is the one
// the catalogue reports.

/// `System.Windows.Forms.View` — how a `ListView` arranges its items.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    #[default]
    LargeIcon,
    Details,
    SmallIcon,
    List,
    Tile,
}

/// `SortOrder` — the direction `ListView.Sorting` applies (also the shape a
/// column's sort glyph would take).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    #[default]
    None,
    Ascending,
    Descending,
}

/// `ColumnHeaderStyle` — whether the Details header is shown and clickable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColumnHeaderStyle {
    /// No header band at all (rows start at the top).
    None,
    Nonclickable,
    #[default]
    Clickable,
}

/// `ItemActivation` — how many clicks activate an item. Stored for fidelity;
/// activation is an input concern the host drives, not a paint concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ItemActivation {
    #[default]
    Standard,
    OneClick,
    TwoClick,
}

/// `ListViewAlignment` — how icons snap in the icon views. The catalogue's
/// default is `Top`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ListViewAlignment {
    Default,
    #[default]
    Top,
    Left,
    SnapToGrid,
}

/// `TreeViewDrawMode` — normal vs owner-draw. Owner-draw is deferred (this port
/// always paints itself), so only `Normal` changes anything today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TreeViewDrawMode {
    #[default]
    Normal,
    OwnerDrawText,
    OwnerDrawAll,
}

// `HorizontalAlignment` is NOT declared here: more than one family needs it
// (`TextBox.TextAlign`, `UpDownBase`, and this file's `ColumnHeader.TextAlign`),
// so it lives once in `enums.rs` — along with its DirectWrite mapping. Its
// default is `Left`, which is exactly what `ColumnHeader.TextAlign` requires.

// ── Shared paint / geometry constants (DIP) ──────────────────────────────────

/// Pixels a `TreeView` level is indented by default (`Indent`).
const DEFAULT_INDENT: i32 = 19;
/// Default `ItemHeight`. In the toolkit this is derived from the font; the port
/// fixes a sensible DIP value and lets the DPI scale apply at paint time.
const DEFAULT_ITEM_HEIGHT: i32 = 19;
/// Side of the `[+]` / `[-]` expand box.
///
/// Also, measured, the natural side of the theme's own `TVP_GLYPH` at 96 DPI —
/// the part is TrueSize, so this box is the one it draws into unstretched.
const GLYPH_BOX: f32 = 9.0;
/// Side of the box the header's sort triangle is centred in. The part is
/// TrueSize (9×5 at 96 DPI), so this only has to be big enough to hold it and
/// short enough to keep it at the top of the header item, where a real header
/// puts it.
const SORT_ARROW_BOX: f32 = 11.0;
/// Side of a node/row check box.
const CHECK_BOX: f32 = 13.0;
/// Row height of a Details `ListView` row.
const DETAILS_ROW_HEIGHT: f32 = 21.0;
/// Height of the Details header band.
const DETAILS_HEADER_HEIGHT: f32 = 23.0;
/// `ColumnHeader`'s documented default width.
const DEFAULT_COLUMN_WIDTH: i32 = 60;
/// Column width used to tile items in `View::List`.
const LIST_COLUMN_WIDTH: f32 = 120.0;
/// Left inset of a Details cell's text, header and body alike.
const CELL_PAD_LEFT: f32 = 6.0;
/// Right inset of a Details cell's text — smaller than the left one, as the
/// header control's own margins are.
const CELL_PAD_RIGHT: f32 = 4.0;
/// Left inset of an item's text in `View::List`.
const LIST_PAD_LEFT: f32 = 4.0;
/// How far a selection band overhangs the label it wraps. The toolkit highlights
/// the label's box, not the row, unless `FullRowSelect` is on — so the band has
/// to be measured from the text, and this is the breathing space it leaves.
const LABEL_PAD: f32 = 1.0;

// =============================================================================
//  TreeView
// =============================================================================

/// A path to a node: the child index at each level from a root. Rust cannot hold
/// a borrowed reference to a node *inside* the tree it also owns, so `SelectedNode`
/// and `TopNode` are expressed as stable paths and resolved on demand.
pub type NodePath = Vec<usize>;

/// `System.Windows.Forms.TreeNode` — one node of a `TreeView`'s tree.
///
/// `Checked` is deliberately independent of the children: WinForms does **not**
/// cascade a check to descendants — that is the app's job in `AfterCheck`
/// (verified against Microsoft's docs). [`TreeView::set_node_checked`] therefore
/// touches exactly one node.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TreeNode {
    /// `Name` — the key used by `Nodes[string]`.
    pub name: String,
    pub text: String,
    /// `IsExpanded` — whether this node's children are shown.
    pub expanded: bool,
    /// `Checked` — the box state when `CheckBoxes` is on. Independent per node.
    pub checked: bool,
    /// `Tag` — an arbitrary host payload, a string for the same reason as
    /// `ControlBase::tag`.
    pub tag: Option<String>,
    /// `ImageIndex` / `SelectedImageIndex` into the (host-owned) image list.
    pub image_index: i32,
    pub selected_image_index: i32,
    pub children: Vec<TreeNode>,
}

impl TreeNode {
    /// A leaf node with the given text; indices default to "inherit from the
    /// control" (`-1`), matching the toolkit.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            name: String::new(),
            text: text.into(),
            expanded: false,
            checked: false,
            tag: None,
            image_index: -1,
            selected_image_index: -1,
            children: Vec::new(),
        }
    }

    /// Builder: append a child and return `self`, for terse tree literals.
    pub fn child(mut self, node: TreeNode) -> Self {
        self.children.push(node);
        self
    }

    /// Builder: start expanded.
    pub fn expanded(mut self) -> Self {
        self.expanded = true;
        self
    }
}

/// One entry of the flattened, currently-visible tree — the pure result the
/// painter and hit-tester walk. This is the same shape as the admin nav tree
/// already shipped in the product: a list of (path, indent depth) pairs.
#[derive(Debug, Clone, PartialEq)]
pub struct VisibleRow {
    pub path: NodePath,
    /// Indent level, root nodes at `0`.
    pub depth: usize,
    pub has_children: bool,
    pub expanded: bool,
    pub checked: bool,
}

/// Flattens `nodes` to the rows that are visible given the expand state — a pure
/// function of the tree alone, so it is fully testable without a window. A node
/// is visible when every ancestor is expanded; a collapsed node contributes its
/// own row but none of its subtree.
pub fn visible_rows(nodes: &[TreeNode]) -> Vec<VisibleRow> {
    fn walk(nodes: &[TreeNode], prefix: &mut NodePath, depth: usize, out: &mut Vec<VisibleRow>) {
        for (i, n) in nodes.iter().enumerate() {
            prefix.push(i);
            out.push(VisibleRow {
                path: prefix.clone(),
                depth,
                has_children: !n.children.is_empty(),
                expanded: n.expanded,
                checked: n.checked,
            });
            // Only descend through an expanded parent — this is exactly what
            // makes the row list "visible" rather than "all".
            if n.expanded && !n.children.is_empty() {
                walk(&n.children, prefix, depth + 1, out);
            }
            prefix.pop();
        }
    }
    let mut out = Vec::new();
    walk(nodes, &mut Vec::new(), 0, &mut out);
    out
}

/// The properties `TreeView` itself declares (34 in the catalogue), minus the
/// four re-declared from `Control` (`BackColor`/`ForeColor`/`Text`/
/// `BackgroundImageLayout`), which are reached through `Deref`.
///
/// `ImageList`/`StateImageList` are not modelled (no `ImageList` port type);
/// `TreeViewNodeSorter` (an `IComparer`) is not modelled — see the module doc.
// No `Debug`/`PartialEq`: `line_color` is a `D2D1_COLOR_F`, which the drawing
// layer does not derive them for.
#[derive(Clone)]
pub struct TreeView {
    control: ControlBase,

    // ── Content ──────────────────────────────────────────────────────────
    /// `Nodes` — the root nodes.
    pub nodes: Vec<TreeNode>,
    /// `SelectedNode`, as a path (see [`NodePath`]). `None` = nothing selected.
    pub selected_path: Option<NodePath>,
    /// `TopNode` — the node scrolled to the top, as a path. `None` = the first
    /// visible row.
    pub top_path: Option<NodePath>,

    // ── Metrics ──────────────────────────────────────────────────────────
    /// `Indent` — DIP added per level.
    pub indent: i32,
    /// `ItemHeight` — row height in DIP. Derived from the font in the toolkit;
    /// see [`DEFAULT_ITEM_HEIGHT`].
    pub item_height: i32,

    // ── Structure lines & glyphs ─────────────────────────────────────────
    pub show_lines: bool,
    pub show_root_lines: bool,
    pub show_plus_minus: bool,
    /// `LineColor`. `None` means « the colour the control uses by default »,
    /// which the reference sheet shows is `GrayText` (`#6D6D6D` on this machine)
    /// — the property's documented default of `Color.Black` is what the
    /// *managed* getter reports, not what comctl32 draws with.
    pub line_color: Option<D2D1_COLOR_F>,

    // ── Behaviour ────────────────────────────────────────────────────────
    pub check_boxes: bool,
    /// `HideSelection` — hide the selection when the control loses focus. Stored,
    /// but not simulated: a pure value has no focus, so the painter treats the
    /// tree as focused and always shows the selection.
    pub hide_selection: bool,
    pub full_row_select: bool,
    pub label_edit: bool,
    /// `HotTracking` — underline nodes on hover. Stored; hover is an input state
    /// this value does not carry.
    pub hot_tracking: bool,
    pub scrollable: bool,
    pub show_node_tool_tips: bool,
    pub right_to_left_layout: bool,
    /// `Sorted` — whether nodes are kept alphabetically by `Text`. This value is
    /// not auto-maintained on insert (it is a plain `Vec`); call [`TreeView::sort`].
    pub sorted: bool,
    pub border_style: BorderStyle,
    /// `DrawMode` — owner-draw is deferred (this port always paints itself).
    pub draw_mode: TreeViewDrawMode,
    /// `PathSeparator` — the string joining texts in [`TreeView::full_path`].
    pub path_separator: String,

    // ── Image indices (the lists themselves are host-resolved) ───────────
    pub image_index: i32,
    pub image_key: String,
    pub selected_image_index: i32,
    pub selected_image_key: String,
}

impl Default for TreeView {
    /// The catalogue's declared defaults: `ShowLines`/`ShowRootLines`/
    /// `ShowPlusMinus`/`Scrollable` = true, `HideSelection` = true (unlike
    /// `ListView`), `CheckBoxes`/`FullRowSelect`/`LabelEdit`/`Sorted` = false,
    /// `PathSeparator` = "\\", `BorderStyle` = `Fixed3D`, image indices = -1.
    fn default() -> Self {
        Self {
            control: ControlBase::new(),
            nodes: Vec::new(),
            selected_path: None,
            top_path: None,
            indent: DEFAULT_INDENT,
            item_height: DEFAULT_ITEM_HEIGHT,
            show_lines: true,
            show_root_lines: true,
            show_plus_minus: true,
            line_color: None,
            check_boxes: false,
            hide_selection: true,
            full_row_select: false,
            label_edit: false,
            hot_tracking: false,
            scrollable: true,
            show_node_tool_tips: false,
            right_to_left_layout: false,
            sorted: false,
            border_style: BorderStyle::Fixed3D,
            draw_mode: TreeViewDrawMode::default(),
            path_separator: "\\".to_string(),
            image_index: -1,
            image_key: String::new(),
            selected_image_index: -1,
            selected_image_key: String::new(),
        }
    }
}

impl Deref for TreeView {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.control
    }
}
impl DerefMut for TreeView {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
}

impl TreeView {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves a [`NodePath`] to a node, or `None` if any index is out of range.
    pub fn node_at<'a>(nodes: &'a [TreeNode], path: &[usize]) -> Option<&'a TreeNode> {
        let mut cur = nodes;
        let mut found = None;
        for &idx in path {
            let n = cur.get(idx)?;
            found = Some(n);
            cur = &n.children;
        }
        found
    }

    /// Mutable twin of [`TreeView::node_at`].
    pub fn node_at_mut<'a>(nodes: &'a mut [TreeNode], path: &[usize]) -> Option<&'a mut TreeNode> {
        let mut cur = nodes;
        for (depth, &idx) in path.iter().enumerate() {
            let n = cur.get_mut(idx)?;
            if depth + 1 == path.len() {
                return Some(n);
            }
            cur = &mut n.children;
        }
        None
    }

    /// `SelectedNode` as a borrow.
    pub fn selected_node(&self) -> Option<&TreeNode> {
        self.selected_path
            .as_ref()
            .and_then(|p| Self::node_at(&self.nodes, p))
    }

    /// `TreeNode.FullPath` — every ancestor's `Text` from the root, joined by
    /// `PathSeparator`. Returns `None` for an empty or invalid path. The root
    /// text is included, exactly as the toolkit composes it.
    pub fn full_path(&self, path: &[usize]) -> Option<String> {
        if path.is_empty() {
            return None;
        }
        let mut parts = Vec::with_capacity(path.len());
        let mut cur = &self.nodes[..];
        for &idx in path {
            let n = cur.get(idx)?;
            parts.push(n.text.as_str());
            cur = &n.children;
        }
        Some(parts.join(&self.path_separator))
    }

    /// Sets one node's `Checked` state. Deliberately does **not** cascade to
    /// children or parents: the WinForms `TreeView` leaves them untouched and
    /// expects the app to cascade in `AfterCheck`.
    pub fn set_node_checked(&mut self, path: &[usize], value: bool) {
        if let Some(n) = Self::node_at_mut(&mut self.nodes, path) {
            n.checked = value;
        }
    }

    /// Sorts nodes recursively by `Text`. Honours `Sorted` only when asked —
    /// this value does not intercept inserts the way the collection does.
    pub fn sort(&mut self) {
        fn rec(nodes: &mut [TreeNode]) {
            for n in nodes.iter_mut() {
                rec(&mut n.children);
            }
            // `sort_by` after recursing keeps children sorted within each parent.
            nodes.sort_by(|a, b| a.text.cmp(&b.text));
        }
        rec(&mut self.nodes);
    }

    /// The flattened visible rows — [`visible_rows`] over the roots.
    pub fn visible_rows(&self) -> Vec<VisibleRow> {
        visible_rows(&self.nodes)
    }

    /// Whether the node at `path` has a later sibling.
    ///
    /// This is what decides where a connecting line stops: the toolkit runs the
    /// vertical through a row when the node still has siblings below it, and cuts
    /// it at the row's middle when it is the last one. Painting it without this
    /// test would drop a line off the bottom of every branch.
    ///
    /// It is a free function over the tree rather than a field on [`VisibleRow`]
    /// so the flattening stays the pure, tested value it already is.
    pub fn has_following_sibling(nodes: &[TreeNode], path: &[usize]) -> bool {
        let Some((&last, parents)) = path.split_last() else {
            return false;
        };
        let mut cur = nodes;
        for &idx in parents {
            match cur.get(idx) {
                Some(n) => cur = &n.children,
                None => return false,
            }
        }
        last + 1 < cur.len()
    }

    /// `VisibleCount` — how many whole rows fit in the client height. Read-only
    /// and derived, as in the toolkit.
    pub fn visible_count(&self) -> i32 {
        let ih = self.item_height.max(1) as f32;
        (self.height() / ih).floor().max(0.0) as i32
    }
}

impl Control for TreeView {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }

    /// The width of the widest visible row (indent + glyph + text) and the total
    /// height of the visible rows, inflated by `Padding` — the caller adds
    /// `Margin`.
    ///
    /// Measuring is the one place a family still reaches for `Canvas::formats()`:
    /// `preferred_size` receives a bare [`Canvas`], which carries no
    /// [`crate::system::Visuals`], so the system message font the painter uses is
    /// simply not reachable from here. The two agree closely enough for layout —
    /// and widening the signature would change every family at once.
    fn preferred_size(&self, c: &dyn Canvas) -> Size {
        let rows = self.visible_rows();
        let ih = self.item_height.max(1) as f32;
        let font = &c.formats().body;
        let mut max_w = 0.0f32;
        for r in &rows {
            if let Some(n) = Self::node_at(&self.nodes, &r.path) {
                // (depth + 1) indents leave room for this level's glyph column.
                let w = (r.depth as f32 + 1.0) * self.indent as f32
                    + GLYPH_BOX
                    + if self.check_boxes { CHECK_BOX + 2.0 } else { 0.0 }
                    + c.measure(&n.text, font)
                    + 4.0;
                max_w = max_w.max(w);
            }
        }
        Size::new(
            max_w + self.padding.horizontal(),
            rows.len() as f32 * ih + self.padding.vertical(),
        )
    }

    /// Paints the tree as a resting, **focused** control — see the module doc.
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.paint_with_state(c, bounds, ControlState { focused: true, ..ControlState::default() });
    }

    /// The real painter.
    ///
    /// Order matters and mirrors the toolkit's: the border first (it reports the
    /// interior everything else is laid out in), then the `Window` ground, then
    /// per row — connecting lines, expand box, check box, selection band, label.
    /// The selection goes down *after* the lines because the toolkit's highlight
    /// covers the horizontal stub that runs under the label.
    fn paint_with_state(&self, c: &dyn ControlCanvas, bounds: Rect, state: ControlState) {
        let v = c.visuals();
        let colors = v.colors;
        // The ground is resolved before the frame is drawn: the themed frame
        // repaints its own inner pad, and the colour that ring must take is this
        // one — see [`paint_border`].
        let ground = self
            .control
            .back_color
            .unwrap_or(if self.enabled { colors.window } else { colors.control });
        let interior = paint_border(c, bounds, self.border_style, self.enabled, &ground);
        c.fill_rect(&interior, &ground);

        let text_color = if self.enabled {
            self.control.fore_color.unwrap_or(colors.window_text)
        } else {
            colors.gray_text
        };
        let line = self.line_color.unwrap_or(colors.gray_text);
        let selection = selection_paint(&colors, state.focused, self.hide_selection);

        let indent = self.indent as f32;
        let ih = self.item_height.max(1) as f32;
        let font = &v.fonts.message;
        let rows = self.visible_rows();

        c.push_clip(&interior);
        for (i, row) in rows.iter().enumerate() {
            let top = interior.top + i as f32 * ih;
            if top >= interior.bottom {
                break;
            }
            let bottom = top + ih;
            let mid_y = top + ih / 2.0;
            // Left edge of this row's content; the glyph column sits just left of
            // it, centred on the level's connecting line.
            let content_left = interior.left + 2.0 + (row.depth as f32 + 1.0) * indent;
            let connector_x = content_left - indent / 2.0;
            // `ShowRootLines = false` takes the root level's lines AND its expand
            // boxes away — the toolkit hides both together.
            let level_has_chrome = row.depth > 0 || self.show_root_lines;

            if self.show_lines {
                // Every ancestor that still has a sibling below keeps its line
                // running through this row: that is what makes a deep branch read
                // as a branch rather than as loose stubs.
                for k in 0..row.depth {
                    if k == 0 && !self.show_root_lines {
                        continue;
                    }
                    if Self::has_following_sibling(&self.nodes, &row.path[..=k]) {
                        let x = interior.left + 2.0 + (k as f32 + 0.5) * indent;
                        dotted_v(c, x, top, bottom, &line);
                    }
                }
                if level_has_chrome {
                    // The very first root has nothing above it, so its line starts
                    // at its own middle; every other node is joined to whatever
                    // precedes it.
                    let first_root = row.depth == 0 && row.path.first() == Some(&0);
                    if !first_root {
                        dotted_v(c, connector_x, top, mid_y, &line);
                    }
                    if Self::has_following_sibling(&self.nodes, &row.path) {
                        dotted_v(c, connector_x, mid_y, bottom, &line);
                    }
                    dotted_h(c, connector_x, content_left, mid_y, &line);
                }
            }

            // Expand box, over the lines it interrupts.
            if row.has_children && self.show_plus_minus && level_has_chrome {
                let g = Rect::new(
                    connector_x - GLYPH_BOX / 2.0,
                    mid_y - GLYPH_BOX / 2.0,
                    connector_x + GLYPH_BOX / 2.0,
                    mid_y + GLYPH_BOX / 2.0,
                );
                draw_expand_glyph(c, &g, row.expanded, &ground);
            }

            let mut text_x = content_left;
            if self.check_boxes {
                let b = Rect::new(
                    text_x,
                    mid_y - CHECK_BOX / 2.0,
                    text_x + CHECK_BOX,
                    mid_y + CHECK_BOX / 2.0,
                );
                draw_check_box(c, &b, row.checked, self.enabled);
                text_x += CHECK_BOX + 3.0;
            }

            let label = Self::node_at(&self.nodes, &row.path).map(|n| n.text.as_str()).unwrap_or("");
            // The highlight wraps the LABEL, not the row — unless `FullRowSelect`
            // says otherwise — so it has to be measured from the text.
            let label_rect = if self.full_row_select {
                Rect::new(interior.left, top, interior.right, bottom)
            } else {
                Rect::new(
                    text_x - LABEL_PAD,
                    top,
                    text_x + c.measure(label, font) + LABEL_PAD,
                    bottom,
                )
            };

            let selected = self.selected_path.as_deref() == Some(row.path.as_slice());
            let band = if selected { selection } else { None };
            let mut fore = text_color;
            if let Some((back, on_back)) = band {
                c.fill_rect(&label_rect, &back);
                fore = on_back;
            }
            c.text_ellipsis(label, &Rect::new(text_x, top, interior.right - 2.0, bottom), font, &fore);
            // The focus rectangle belongs to the focused control alone; an
            // unfocused tree shows the band without it.
            if selected && state.focused {
                dotted_focus_rect(c, &label_rect, &colors.window_text);
            }
        }
        c.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "TreeView"
    }
}

// =============================================================================
//  ListView
// =============================================================================

/// `ListViewItem.ListViewSubItem` — a cell in a Details row past the first
/// column. Per-sub-item colours/font are deferred (kept simple: a value is its
/// text); the row's own colours drive painting.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListViewSubItem {
    pub text: String,
    pub name: String,
    pub tag: Option<String>,
}

impl ListViewSubItem {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            name: String::new(),
            tag: None,
        }
    }
}

/// `System.Windows.Forms.ListViewItem` — a row.
///
/// `Text` is the first column; `sub_items` are columns 1.. (the toolkit stores
/// the item text as `SubItems[0]`, but keeping `text` explicit is clearer and
/// the Details painter maps column 0 → `text`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListViewItem {
    pub name: String,
    pub text: String,
    pub sub_items: Vec<ListViewSubItem>,
    /// `Checked` — the box state when `CheckBoxes` is on.
    pub checked: bool,
    /// `Selected` — the toolkit derives `SelectedIndices` from this per-item
    /// flag, so the flag is the source of truth here too.
    pub selected: bool,
    pub tag: Option<String>,
    pub image_index: i32,
    pub state_image_index: i32,
    /// `Group` — index into [`ListView::groups`], or `None` for the default group.
    pub group: Option<usize>,
    /// `IndentCount` — Details indent, in whole small-image widths.
    pub indent_count: i32,
}

impl ListViewItem {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            name: String::new(),
            text: text.into(),
            sub_items: Vec::new(),
            checked: false,
            selected: false,
            tag: None,
            image_index: -1,
            state_image_index: 0,
            group: None,
            indent_count: 0,
        }
    }

    /// Builder: append a sub-item (a further Details column) and return `self`.
    pub fn with_sub(mut self, text: impl Into<String>) -> Self {
        self.sub_items.push(ListViewSubItem::new(text));
        self
    }

    /// The text shown in Details column `col` (0 = `text`, k = `sub_items[k-1]`).
    pub fn cell(&self, col: usize) -> &str {
        if col == 0 {
            &self.text
        } else {
            self.sub_items
                .get(col - 1)
                .map(|s| s.text.as_str())
                .unwrap_or("")
        }
    }
}

/// `System.Windows.Forms.ColumnHeader` — a Details column.
#[derive(Debug, Clone, PartialEq)]
pub struct ColumnHeader {
    pub name: String,
    pub text: String,
    /// `Width` in DIP. WinForms' magic `-1` (auto-fit content) / `-2` (auto-fit
    /// header) are not resolved here; a caller wanting them must compute a width.
    pub width: i32,
    /// `TextAlign`. Note the toolkit forces column 0 to `Left`; the painter
    /// enforces that.
    pub text_align: HorizontalAlignment,
    pub tag: Option<String>,
    pub display_index: i32,
}

impl ColumnHeader {
    pub fn new(text: impl Into<String>, width: i32) -> Self {
        Self {
            name: String::new(),
            text: text.into(),
            width,
            text_align: HorizontalAlignment::Left,
            tag: None,
            display_index: 0,
        }
    }
}

impl Default for ColumnHeader {
    fn default() -> Self {
        Self::new(String::new(), DEFAULT_COLUMN_WIDTH)
    }
}

/// `System.Windows.Forms.ListViewGroup` — a titled band of items.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ListViewGroup {
    pub name: String,
    pub header: String,
    pub header_alignment: HorizontalAlignment,
    /// `Collapsed` — whether the group is folded. Stored; folding is not painted.
    pub collapsed: bool,
    pub tag: Option<String>,
}

impl ListViewGroup {
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            name: String::new(),
            header: header.into(),
            header_alignment: HorizontalAlignment::Left,
            collapsed: false,
            tag: None,
        }
    }
}

/// The Details-view geometry, resolved from a `ListView` and its painted bounds.
/// Every field is a plain number so column arithmetic and hit-testing are pure
/// and testable without a device.
// No `Debug`/`PartialEq`: `content` is a `Rect`, which the drawing layer does
// not derive them for. It is `Copy` so callers pass it by value freely.
#[derive(Clone, Copy)]
pub struct DetailsGeometry {
    /// The area inside the border where header + rows are laid out.
    pub content: Rect,
    /// Header band height, `0.0` when the header is hidden.
    pub header_height: f32,
    pub row_height: f32,
}

impl DetailsGeometry {
    /// The left x of each column, plus a final entry at the right edge of the
    /// last column — so column `i` spans `xs[i]..xs[i+1]`.
    pub fn column_x_offsets(&self, columns: &[ColumnHeader]) -> Vec<f32> {
        let mut xs = Vec::with_capacity(columns.len() + 1);
        let mut x = self.content.left;
        xs.push(x);
        for col in columns {
            x += col.width.max(0) as f32;
            xs.push(x);
        }
        xs
    }

    /// The rectangle of Details row `index` (0-based, past the header).
    pub fn row_rect(&self, index: usize) -> Rect {
        let top = self.content.top + self.header_height + index as f32 * self.row_height;
        Rect::new(self.content.left, top, self.content.right, top + self.row_height)
    }

    /// Hit-tests a point to `(item index, sub-item/column index)`. Returns `None`
    /// for the header band, empty space past the last row, or a point outside a
    /// column's horizontal span.
    pub fn hit_test(
        &self,
        columns: &[ColumnHeader],
        item_count: usize,
        x: f32,
        y: f32,
    ) -> Option<(usize, usize)> {
        if self.row_height <= 0.0 || !self.content.contains(x, y) {
            return None;
        }
        let body_top = self.content.top + self.header_height;
        if y < body_top {
            return None; // in the header
        }
        let row = ((y - body_top) / self.row_height) as usize;
        if row >= item_count {
            return None;
        }
        let xs = self.column_x_offsets(columns);
        // `.windows(2)` gives each [left, right) pair without indexing tricks.
        for (col, w) in xs.windows(2).enumerate() {
            if x >= w[0] && x < w[1] {
                return Some((row, col));
            }
        }
        None
    }
}

/// Tiles `item_count` items top-to-bottom then wrapping into the next column —
/// the layout `View::List` uses. Pure, so wrapping is testable.
pub fn list_column_layout(
    item_count: usize,
    col_width: f32,
    row_height: f32,
    area: Rect,
) -> Vec<Rect> {
    let per_col = ((area.bottom - area.top) / row_height).floor().max(1.0) as usize;
    (0..item_count)
        .map(|i| {
            let col = i / per_col;
            let row = i % per_col;
            let l = area.left + col as f32 * col_width;
            let t = area.top + row as f32 * row_height;
            Rect::new(l, t, l + col_width, t + row_height)
        })
        .collect()
}

/// The properties `ListView` itself declares (41 in the catalogue), minus the
/// four re-declared from `Control` reached through `Deref`. Image lists and
/// `ListViewItemSorter` are not modelled — see the module doc.
// No `Debug`: contains no colour, but kept consistent with `TreeView`; it may
// derive `Clone` freely.
#[derive(Clone)]
pub struct ListView {
    control: ControlBase,

    // ── Content ──────────────────────────────────────────────────────────
    pub items: Vec<ListViewItem>,
    pub columns: Vec<ColumnHeader>,
    pub groups: Vec<ListViewGroup>,

    // ── View mode ────────────────────────────────────────────────────────
    pub view: View,
    pub alignment: ListViewAlignment,
    /// `TileSize`. `Size::EMPTY` means "derive from the font/icon", as the
    /// toolkit does when it is unset.
    pub tile_size: Size,

    // ── Selection / checks ───────────────────────────────────────────────
    pub multi_select: bool,
    pub check_boxes: bool,
    /// `FocusedItem` as an index. Settable in the toolkit; stored as an index.
    pub focused_index: Option<usize>,
    /// `TopItem` — first visible row, as an index.
    pub top_index: Option<usize>,
    pub full_row_select: bool,
    /// `HideSelection` (default *false* for `ListView`, unlike `TreeView`).
    /// Stored, not simulated — same reasoning as `TreeView`.
    pub hide_selection: bool,

    // ── Chrome / behaviour ───────────────────────────────────────────────
    pub grid_lines: bool,
    pub header_style: ColumnHeaderStyle,
    pub border_style: BorderStyle,
    pub label_edit: bool,
    pub label_wrap: bool,
    pub allow_column_reorder: bool,
    pub auto_arrange: bool,
    pub scrollable: bool,
    pub hot_tracking: bool,
    pub hover_selection: bool,
    pub show_groups: bool,
    pub show_item_tool_tips: bool,
    pub right_to_left_layout: bool,
    pub background_image_tiled: bool,
    pub activation: ItemActivation,
    /// `Sorting` — direction items are kept in by `Text`. Not auto-maintained;
    /// call [`ListView::sort`].
    pub sorting: SortOrder,
    /// `OwnerDraw` — deferred; this port always paints itself.
    pub owner_draw: bool,

    // ── Virtual mode ─────────────────────────────────────────────────────
    /// `VirtualMode`. Modelled as a flag + count: this port does **not** fetch
    /// items on demand (there is no `RetrieveVirtualItem` callback in a pure
    /// value), so in virtual mode `items` is expected to be empty and only
    /// `virtual_list_size` drives [`ListView::item_count`] and the row geometry.
    pub virtual_mode: bool,
    pub virtual_list_size: i32,
}

impl Default for ListView {
    /// The catalogue's declared defaults: `View` = `LargeIcon`, `MultiSelect` =
    /// true, `AutoArrange`/`LabelWrap`/`Scrollable`/`ShowGroups` = true,
    /// `HideSelection` = false, `HeaderStyle` = `Clickable`, `Sorting` = `None`,
    /// `Alignment` = `Top`, `Activation` = `Standard`, `VirtualListSize` = 0,
    /// `BorderStyle` = `Fixed3D`; everything else false.
    fn default() -> Self {
        Self {
            control: ControlBase::new(),
            items: Vec::new(),
            columns: Vec::new(),
            groups: Vec::new(),
            view: View::default(),
            alignment: ListViewAlignment::default(),
            tile_size: Size::EMPTY,
            multi_select: true,
            check_boxes: false,
            focused_index: None,
            top_index: None,
            full_row_select: false,
            hide_selection: false,
            grid_lines: false,
            header_style: ColumnHeaderStyle::default(),
            border_style: BorderStyle::Fixed3D,
            label_edit: false,
            label_wrap: true,
            allow_column_reorder: false,
            auto_arrange: true,
            scrollable: true,
            hot_tracking: false,
            hover_selection: false,
            show_groups: true,
            show_item_tool_tips: false,
            right_to_left_layout: false,
            background_image_tiled: false,
            activation: ItemActivation::default(),
            sorting: SortOrder::default(),
            owner_draw: false,
            virtual_mode: false,
            virtual_list_size: 0,
        }
    }
}

impl Deref for ListView {
    type Target = ControlBase;
    fn deref(&self) -> &ControlBase {
        &self.control
    }
}
impl DerefMut for ListView {
    fn deref_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }
}

impl ListView {
    pub fn new() -> Self {
        Self::default()
    }

    /// The number of rows to reason about: `VirtualListSize` in virtual mode,
    /// else the concrete item count.
    pub fn item_count(&self) -> usize {
        if self.virtual_mode {
            self.virtual_list_size.max(0) as usize
        } else {
            self.items.len()
        }
    }

    /// `SelectedIndices` — derived from each item's `Selected` flag.
    pub fn selected_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.selected)
            .map(|(i, _)| i)
            .collect()
    }

    /// `CheckedIndices` — derived from each item's `Checked` flag.
    pub fn checked_indices(&self) -> Vec<usize> {
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| it.checked)
            .map(|(i, _)| i)
            .collect()
    }

    /// `SelectedItems` — the SAME selection as [`ListView::selected_indices`],
    /// read as items instead of positions.
    ///
    /// An accessor, deliberately not a field: the toolkit's two collections are
    /// two views of one fact, and storing the fact twice is how the two readings
    /// drift apart. Borrowing rather than cloning also keeps it free to call.
    pub fn selected_items(&self) -> impl Iterator<Item = &ListViewItem> {
        self.items.iter().filter(|it| it.selected)
    }

    /// `CheckedItems` — the item-valued twin of [`ListView::checked_indices`],
    /// for the same reason as [`ListView::selected_items`].
    ///
    /// Note it is a real toolkit member but is **not** in the reflection
    /// catalogue's declared set (it is not designer-visible), so the coverage
    /// audit does not require it; it exists here for symmetry with the
    /// selection pair, which is what a caller expects.
    pub fn checked_items(&self) -> impl Iterator<Item = &ListViewItem> {
        self.items.iter().filter(|it| it.checked)
    }

    /// Selects (or deselects) an item, enforcing `MultiSelect`: with it off,
    /// selecting one item clears every other — the toolkit's own rule.
    pub fn set_selected(&mut self, index: usize, selected: bool) {
        if index >= self.items.len() {
            return;
        }
        if selected && !self.multi_select {
            for (i, it) in self.items.iter_mut().enumerate() {
                it.selected = i == index;
            }
        } else {
            self.items[index].selected = selected;
        }
    }

    /// Sorts items by `Text` in the `Sorting` direction. Like `TreeView::sort`,
    /// this is explicit rather than intercepting inserts.
    pub fn sort(&mut self) {
        match self.sorting {
            SortOrder::None => {}
            SortOrder::Ascending => self.items.sort_by(|a, b| a.text.cmp(&b.text)),
            SortOrder::Descending => self.items.sort_by(|a, b| b.text.cmp(&a.text)),
        }
    }

    /// Builds the Details geometry for a painted `bounds`.
    pub fn details_geometry(&self, bounds: Rect) -> DetailsGeometry {
        let inset = if self.border_style == BorderStyle::None { 0.0 } else { 1.0 };
        let content = Rect::new(
            bounds.left + inset,
            bounds.top + inset,
            bounds.right - inset,
            bounds.bottom - inset,
        );
        let header_height =
            if self.header_style == ColumnHeaderStyle::None || self.view != View::Details {
                0.0
            } else {
                DETAILS_HEADER_HEIGHT
            };
        DetailsGeometry {
            content,
            header_height,
            row_height: DETAILS_ROW_HEIGHT,
        }
    }

    // ── Painting helpers, split by view ──────────────────────────────────

    /// The colour a row's text is painted in when it is not selected — the
    /// control's own `ForeColor` if one was set, else `WindowText`, else the one
    /// colour a disabled control greys its text with.
    fn text_color(&self, colors: &SystemColors) -> D2D1_COLOR_F {
        if self.enabled {
            self.control.fore_color.unwrap_or(colors.window_text)
        } else {
            colors.gray_text
        }
    }

    /// The colour behind everything the control draws — its own `BackColor` if
    /// one was set, else `Window`, or `Control` when it is disabled.
    ///
    /// Resolved in one place because three callers must agree on it: the fill,
    /// the themed frame (which repaints its inner pad in it) and the themed
    /// header (which is composited onto it). Two of the three would otherwise be
    /// guessing at what the first had put there.
    fn ground(&self, colors: &SystemColors) -> D2D1_COLOR_F {
        self.control
            .back_color
            .unwrap_or(if self.enabled { colors.window } else { colors.control })
    }

    /// `View::Details` — the header band, then the rows, then the grid.
    ///
    /// ## The header, themed and classic
    ///
    /// Themed, every column is one `HP_HEADERITEM`: it paints the sheet's
    /// `#FFFFFF` face and the `#E5E5E5` rule down its **own right edge**, so the
    /// separator between two columns is the left column's last pixel and the
    /// port draws no line of its own. Neither colour is a `SystemColors` entry,
    /// which is why the band used to be `Control` grey with a bevel per column
    /// and is now the real thing.
    ///
    /// The strip past the last column is part of the header band — the sheet
    /// shows it white up to the control's own frame, with **no** rule at its
    /// right edge. It is painted as one more `HP_HEADERITEM` stretched a device
    /// pixel past the band and clipped to it, which puts that item's rule
    /// outside the visible area instead of drawing a separator against the
    /// frame. Filling it with `Window` would have been a coincidence, not a
    /// construction: the two are both `#FFFFFF` today and are different
    /// questions.
    ///
    /// Classic, the band is a `Control` face with a raised
    /// [`ControlCanvas::draw_edge`] box per column, which is what a themed-off
    /// machine actually shows.
    ///
    /// ## What is not painted
    ///
    /// `CheckBoxes` is **not** painted here: the state-image column it adds
    /// shifts every cell, which is geometry the model does not carry.
    ///
    /// The header items are always `HIS_NORMAL`. `HIS_HOT` and `HIS_PRESSED` are
    /// measured and pinned, but nothing can reach them honestly: hotness is
    /// per-**column**, and the only pointer state a control is given is
    /// [`ControlState`] for the control as a whole — wiring that through would
    /// light every column at once because the pointer is over a row three
    /// hundred pixels below. A `ColumnHeader` gaining a hover flag is an
    /// item-model change, not a rendering one.
    fn paint_details(&self, c: &dyn ControlCanvas, geo: DetailsGeometry, state: ControlState) {
        let v = c.visuals();
        let colors = v.colors;
        let font = &v.fonts.message;
        let xs = geo.column_x_offsets(&self.columns);
        let text_color = self.text_color(&colors);
        let ground = self.ground(&colors);
        let selection = selection_paint(&colors, state.focused, self.hide_selection);

        if geo.header_height > 0.0 {
            let band = Rect::new(
                geo.content.left,
                geo.content.top,
                geo.content.right,
                geo.content.top + geo.header_height,
            );
            let t = device_pixel(c);
            // The band's own ground, under the columns: one header item stretched
            // one device pixel past the right edge, clipped back to the band, so
            // its rule lands outside. Whether it took is also how the loop below
            // knows which branch to draw the columns in — one question asked once,
            // rather than a themed face under a classic bevel.
            c.push_clip(&band);
            let themed = c.draw_theme_part(
                theme::class::HEADER,
                part::HP_HEADERITEM,
                part::HIS_NORMAL,
                Rect::new(band.left, band.top, band.right + t, band.bottom),
                ground,
            );
            if !themed {
                c.fill_rect(&band, &colors.control);
            }
            for (i, col) in self.columns.iter().enumerate() {
                let item = Rect::new(xs[i], band.top, xs[i + 1], band.bottom);
                // The themed item has no bevel at all — its only chrome is the
                // rule on its last pixel column — so its interior is the item
                // less that one pixel, where the classic raised box gives up two
                // rings on every side.
                let inner = if themed {
                    c.draw_theme_part(
                        theme::class::HEADER,
                        part::HP_HEADERITEM,
                        part::HIS_NORMAL,
                        item,
                        ground,
                    );
                    Rect::new(item.left, item.top, (item.right - t).max(item.left), item.bottom)
                } else {
                    c.draw_edge(&item, Border3DStyle::Raised, Border3DSide::ALL)
                };
                let cell = Rect::new(
                    inner.left + CELL_PAD_LEFT,
                    inner.top,
                    (inner.right - CELL_PAD_RIGHT).max(inner.left + CELL_PAD_LEFT),
                    inner.bottom,
                );
                let align = details_column_align(i, col.text_align);
                c.text_aligned(&col.text, &cell, font, &colors.control_text, align.dwrite());
                if i == 0 {
                    draw_sort_arrow(c, &inner, self.sorting, &ground);
                }
            }
            c.pop_clip();
        }

        let last_column_right = *xs.last().unwrap_or(&geo.content.left);
        for idx in 0..self.item_count() {
            let row = geo.row_rect(idx);
            if row.top >= geo.content.bottom {
                break;
            }
            let item = self.items.get(idx);
            let selected = item.map(|it| it.selected).unwrap_or(false);

            // `FullRowSelect` highlights from the first column to the last one's
            // right edge — NOT to the control's edge; the strip past the columns
            // stays `Window`. Without it, only the first column's label is
            // highlighted, which is why the band has to be measured.
            let mut selected_fore = None;
            if let Some((back, on_back)) = if selected { selection } else { None } {
                let band = if self.full_row_select {
                    Rect::new(xs[0], row.top, last_column_right, row.bottom)
                } else {
                    let label = item.map(|it| it.cell(0)).unwrap_or("");
                    let left = xs[0] + CELL_PAD_LEFT;
                    Rect::new(
                        left - LABEL_PAD,
                        row.top,
                        left + c.measure(label, font) + LABEL_PAD,
                        row.bottom,
                    )
                };
                c.fill_rect(&band, &back);
                selected_fore = Some(on_back);
            }

            for (col, header) in self.columns.iter().enumerate() {
                let cell = Rect::new(
                    xs[col] + CELL_PAD_LEFT,
                    row.top,
                    (xs[col + 1] - CELL_PAD_RIGHT).max(xs[col] + CELL_PAD_LEFT),
                    row.bottom,
                );
                let text = item.map(|it| it.cell(col)).unwrap_or("");
                let align = details_column_align(col, header.text_align);
                // Only the cells the band actually covers get the selected ink.
                let fore = match selected_fore {
                    Some(f) if self.full_row_select || col == 0 => f,
                    _ => text_color,
                };
                c.text_aligned(text, &cell, font, &fore, align.dwrite());
            }

            // `GridLines`: `Control`, read off the reference sheet (`#F0F0F0`),
            // one device pixel — the row's own last scan line horizontally, the
            // next column's first one vertically, exactly as the toolkit lays
            // them.
            if self.grid_lines {
                grid_h(c, geo.content.left, geo.content.right, row.bottom, &colors.control);
                for x in xs.iter().skip(1) {
                    grid_v(c, *x, row.top, row.bottom, &colors.control);
                }
            }
        }
    }

    /// `View::List` — items tiled top-down then wrapping into the next column,
    /// each with a label-width highlight.
    fn paint_list(&self, c: &dyn ControlCanvas, content: Rect, state: ControlState) {
        let v = c.visuals();
        let colors = v.colors;
        let font = &v.fonts.message;
        let text_color = self.text_color(&colors);
        let selection = selection_paint(&colors, state.focused, self.hide_selection);
        let rects =
            list_column_layout(self.item_count(), LIST_COLUMN_WIDTH, DETAILS_ROW_HEIGHT, content);
        for (i, r) in rects.iter().enumerate() {
            let Some(item) = self.items.get(i) else {
                continue;
            };
            let left = r.left + LIST_PAD_LEFT;
            let mut fore = text_color;
            if let Some((back, on_back)) = if item.selected { selection } else { None } {
                let w = c.measure(&item.text, font);
                c.fill_rect(
                    &Rect::new(left - LABEL_PAD, r.top, left + w + LABEL_PAD, r.bottom),
                    &back,
                );
                fore = on_back;
            }
            c.text_ellipsis(&item.text, &Rect::new(left, r.top, r.right - 2.0, r.bottom), font, &fore);
        }
    }

    /// Icon-ish fallback painter (LargeIcon / SmallIcon / Tile): items in a
    /// simple left-to-right, top-to-bottom flow. The distinct icon metrics of
    /// each of those three views are not yet differentiated — documented so the
    /// caller knows the layout is approximate for icon views.
    fn paint_icons(&self, c: &dyn ControlCanvas, content: Rect, state: ControlState) {
        // Reuse the List tiling but flow horizontally-first would need columns;
        // for now share the column tiling so items are at least all visible.
        self.paint_list(c, content, state);
    }
}

impl Control for ListView {
    fn control(&self) -> &ControlBase {
        &self.control
    }
    fn control_mut(&mut self) -> &mut ControlBase {
        &mut self.control
    }

    fn preferred_size(&self, _c: &dyn Canvas) -> Size {
        let p = self.padding;
        match self.view {
            View::Details => {
                let w: f32 = self.columns.iter().map(|col| col.width.max(0) as f32).sum();
                let header = if self.header_style == ColumnHeaderStyle::None {
                    0.0
                } else {
                    DETAILS_HEADER_HEIGHT
                };
                let h = header + self.item_count() as f32 * DETAILS_ROW_HEIGHT;
                Size::new(w + p.horizontal(), h + p.vertical())
            }
            View::List => {
                // A single column's worth is a reasonable minimum.
                let h = self.item_count() as f32 * DETAILS_ROW_HEIGHT;
                Size::new(LIST_COLUMN_WIDTH + p.horizontal(), h + p.vertical())
            }
            // Icon views: fall back to the current box; their true measurement
            // needs icon metrics that are not modelled yet.
            _ => Size::new(self.width().max(120.0), self.height().max(96.0)),
        }
    }

    /// Paints the list as a resting, **focused** control — see the module doc.
    fn paint(&self, c: &dyn ControlCanvas, bounds: Rect) {
        self.paint_with_state(c, bounds, ControlState { focused: true, ..ControlState::default() });
    }

    /// Border, ground, then whichever view is in force.
    ///
    /// The content is clipped to the interior the border reports, but laid out
    /// against [`ListView::details_geometry`], which insets a flat 1 DIP because
    /// it is a pure function of `bounds` with no canvas to ask for the scale. The
    /// two differ by well under a pixel at any scale the shell runs at, and the
    /// clip is what guarantees nothing is ever painted over the bevel.
    fn paint_with_state(&self, c: &dyn ControlCanvas, bounds: Rect, state: ControlState) {
        let colors = c.visuals().colors;
        // Resolved before the frame: the themed frame repaints its own inner pad
        // in this colour — see [`paint_border`].
        let ground = self.ground(&colors);
        let interior = paint_border(c, bounds, self.border_style, self.enabled, &ground);
        c.fill_rect(&interior, &ground);

        let geo = self.details_geometry(bounds);
        c.push_clip(&interior);
        match self.view {
            View::Details => self.paint_details(c, geo, state),
            View::List => self.paint_list(c, geo.content, state),
            View::LargeIcon | View::SmallIcon | View::Tile => {
                self.paint_icons(c, geo.content, state)
            }
        }
        c.pop_clip();
    }

    fn type_name(&self) -> &'static str {
        "ListView"
    }
}

// ── Small shared painting primitives ─────────────────────────────────────────

/// The alignment a Details cell in column `index` actually paints with.
///
/// **This deliberately ignores `TextAlign` on column 0.** It is not an
/// oversight to be tidied away: the Win32 header control cannot right- or
/// centre-align its first column, so WinForms silently forces `Left` there and
/// a `ColumnHeader[0].TextAlign = Right` has no effect in the real toolkit.
/// Honouring it here would make the port diverge from the reference sheet.
fn details_column_align(index: usize, declared: HorizontalAlignment) -> HorizontalAlignment {
    if index == 0 {
        HorizontalAlignment::Left
    } else {
        declared
    }
}

/// One **device** pixel, expressed in the DIP the canvas draws in.
///
/// Every line in this family is one device pixel — a connecting line, a grid
/// line and a focus rectangle are hairlines at 100 % and hairlines at 175 %,
/// which is what makes a `ListView` look like a `ListView` rather than like a
/// scaled picture of one.
fn device_pixel(c: &dyn Canvas) -> f32 {
    1.0 / c.scale().max(0.01)
}

/// The three `BorderStyle` values as the toolkit paints them; returns the
/// **interior** the content is laid out in.
///
/// ## Two renderings of `Fixed3D`, both correct
///
/// With visual styles ON — how the reference sheet was captured — a `Fixed3D`
/// view is framed by a flat theme line, `#ABADB3` on a default Windows 11, with
/// no bevel at all; the sheet shows exactly one such pixel on each side of both
/// the `TreeView` and the `ListView`. With them OFF it is the classic two-ring
/// sunken well `DrawEdge` builds from `COLOR_3DDKSHADOW` and friends. Neither
/// approximates the other and no `GetSysColor` index produces the first, which
/// is what [`crate::theme`] exists for.
///
/// The part is `EP_EDITTEXT`, the same one `TextBoxBase` adopted — so a field
/// and a list keep reading as the same depth, which is the reason
/// [`containers::paint_border`] is shared in the first place. It is **not**
/// `EP_EDITBORDER_NOSCROLL`: measured against the sheet, that one draws Windows
/// 11's modern rounded frame and this one draws the toolkit's.
///
/// ## Why the pad is repainted, and why `ground` is a parameter
///
/// `EP_EDITTEXT` paints a frame **and** a fill, and the fill is not always the
/// one this control wants: under `ETS_DISABLED` the theme tints it (`#B1CEED`
/// here) where the toolkit shows the control's own ground, because a real
/// control's client area covers everything but the frame's rings. So the inner
/// ring is put back explicitly in `ground` — the same colour the caller is about
/// to fill the interior with, which for a view is `Window`, `Control` when it is
/// disabled, or its own `BackColor`. `TextBoxBase` passes `Window` there because
/// a field's client area is always `Window`; a view has three answers, so it
/// hands the one it resolved.
///
/// The **interior is unchanged** by the themed path: it is `bounds` deflated by
/// two device pixels either way — the classic sunken well's two rings, or the
/// theme's one frame pixel plus the pad ring this function repaints. Nothing
/// that lays content out against it has to know which branch ran.
///
/// `None` and `FixedSingle` have no themed form (a `FixedSingle` control is a
/// `WS_BORDER` window, which Windows frames with `COLOR_WINDOWFRAME` whatever
/// the visual style), so both fall straight through to the shared helper.
fn paint_border(
    c: &dyn ControlCanvas,
    bounds: Rect,
    style: BorderStyle,
    enabled: bool,
    ground: &D2D1_COLOR_F,
) -> Rect {
    if style == BorderStyle::Fixed3D {
        // The control's real state, not a convenient one. It changes no pixel of
        // the frame today — the theme draws the same `#ABADB3` line in every
        // state, which the sheet confirms — but passing a lie because it happens
        // not to show would be a fact waiting to be wrong.
        let state = if enabled { ETS_NORMAL } else { ETS_DISABLED };
        if c.draw_theme_part(theme::class::EDIT, EP_EDITTEXT, state, bounds, *ground) {
            c.fill_rect(&edge_interior(&bounds, 1, c.scale()), ground);
            return edge_interior(&bounds, 2, c.scale());
        }
    }
    containers::paint_border(c, bounds, style)
}

/// The sort triangle on a sorted column's header, or nothing when
/// `Sorting = None` — which is the sheet's case, so this draws no pixel there.
///
/// `HP_HEADERSORTARROW`, `HSAS_SORTEDUP` for ascending and `HSAS_SORTEDDOWN` for
/// descending, measured: the two really are a triangle up and a triangle down,
/// in the theme's own greys.
///
/// It goes on **column 0** because that is the column this port sorts by:
/// [`ListView::sort`] orders items by their `Text`, and `Text` is column 0. The
/// toolkit leaves the glyph to the application (a `ListView` never sets it
/// itself), so there is no other column it could belong to here.
///
/// The part is TrueSize — it centres its own 9×5 at 96 DPI in whatever it is
/// given — so it is handed the top band of the header item and left to place
/// itself, which is where a real header puts it. There is no classic branch: a
/// themed-off header has no sort glyph either, because comctl32's is a theme
/// bitmap and the classic header draws none.
fn draw_sort_arrow(
    c: &dyn ControlCanvas,
    inner: &Rect,
    sorting: SortOrder,
    background: &D2D1_COLOR_F,
) {
    let state = match sorting {
        SortOrder::None => return,
        SortOrder::Ascending => part::HSAS_SORTEDUP,
        SortOrder::Descending => part::HSAS_SORTEDDOWN,
    };
    let width = (inner.right - inner.left).min(SORT_ARROW_BOX);
    if width <= 0.0 {
        return;
    }
    let mid_x = (inner.left + inner.right) / 2.0;
    let box_rect = Rect::new(
        mid_x - width / 2.0,
        inner.top,
        mid_x + width / 2.0,
        (inner.top + SORT_ARROW_BOX).min(inner.bottom),
    );
    c.draw_theme_part(
        theme::class::HEADER,
        part::HP_HEADERSORTARROW,
        state,
        box_rect,
        *background,
    );
}

/// The pair of colours a selection band is painted in, or `None` when the
/// toolkit paints no band at all.
///
/// The rule is the real one, and it has three cases, not two:
///
/// * **focused** — `Highlight` under `HighlightText`, the system blue;
/// * **not focused, `HideSelection`** — nothing: the selection is invisible
///   until the control gets the focus back (a `TreeView` defaults to this);
/// * **not focused, not `HideSelection`** — `Control` under `WindowText`, the
///   grey band the reference sheet's `ListView` shows.
///
/// ## Why these are system colours and not `LVP_LISTITEM` / `TVP_TREEITEM`
///
/// Those two parts are the obvious themed answer and they are measurably the
/// wrong one: `IsThemePartDefined` says **false** for both, in every state, on
/// this machine's theme, because Windows Vista moved item rendering to the
/// `Explorer::` subclass that a `ListView`/`TreeView` only gets after an
/// explicit `SetWindowTheme`, which WinForms does not call. Asking anyway does
/// not fail — an undefined part paints a `#FFFFFF` box in a `#828790` frame and
/// reports success — so the check has to be a measurement, not a return value.
///
/// The sheet settles it: its unfocused `ListView` band is flat `#F0F0F0`, which
/// is `Control` exactly, and its focused `TreeView` band is flat `#0078D7`,
/// which is `Highlight` exactly. `Explorer::LISTVIEW`'s `LISS_SELECTEDNOTFOCUS`
/// — which *is* defined — is a rounded `#CCCCCC` with `#8B8B8B` edges, and is
/// neither. See [`crate::views`]'s module docs and the test that pins it.
fn selection_paint(
    colors: &SystemColors,
    focused: bool,
    hide_selection: bool,
) -> Option<(D2D1_COLOR_F, D2D1_COLOR_F)> {
    if focused {
        Some((colors.highlight, colors.highlight_text))
    } else if hide_selection {
        None
    } else {
        Some((colors.control, colors.window_text))
    }
}

/// A Details grid line, horizontal: the one device pixel **ending** at `y`.
///
/// It ends there rather than starting there because the toolkit draws the line
/// on the row's own last scan line — a line *under* the row would make the last
/// row of a full list draw outside itself.
fn grid_h(c: &dyn ControlCanvas, x0: f32, x1: f32, y: f32, color: &D2D1_COLOR_F) {
    let t = device_pixel(c);
    c.fill_rect(&Rect::new(x0, y - t, x1, y), color);
}

/// A Details grid line, vertical: the one device pixel **starting** at `x`, i.e.
/// the first column of the cell to its right, which is where the toolkit puts it.
fn grid_v(c: &dyn ControlCanvas, x: f32, y0: f32, y1: f32, color: &D2D1_COLOR_F) {
    let t = device_pixel(c);
    c.fill_rect(&Rect::new(x, y0, x + t, y1), color);
}

/// The toolkit's dotted connecting line, vertical.
///
/// One device pixel on, one off. The comb is anchored to the **device pixel
/// grid**, not to the segment's own start: a branch is painted as one segment
/// per row, and a per-segment phase would make consecutive rows fall out of step
/// wherever a row height is an odd number of pixels — which is exactly what the
/// reference sheet does *not* show.
fn dotted_v(c: &dyn ControlCanvas, x: f32, y0: f32, y1: f32, color: &D2D1_COLOR_F) {
    let s = c.scale().max(0.01);
    let t = 1.0 / s;
    let x = (x * s).floor() / s;
    let end = (y1 * s).round() as i64;
    let mut i = (y0 * s).round() as i64;
    if i.rem_euclid(2) != 0 {
        i += 1;
    }
    while i < end {
        let y = i as f32 / s;
        c.fill_rect(&Rect::new(x, y, x + t, y + t), color);
        i += 2;
    }
}

/// The toolkit's dotted connecting line, horizontal — see [`dotted_v`].
fn dotted_h(c: &dyn ControlCanvas, x0: f32, x1: f32, y: f32, color: &D2D1_COLOR_F) {
    let s = c.scale().max(0.01);
    let t = 1.0 / s;
    let y = (y * s).floor() / s;
    let end = (x1 * s).round() as i64;
    let mut i = (x0 * s).round() as i64;
    if i.rem_euclid(2) != 0 {
        i += 1;
    }
    while i < end {
        let x = i as f32 / s;
        c.fill_rect(&Rect::new(x, y, x + t, y + t), color);
        i += 2;
    }
}

/// `DrawFocusRect` — the dotted one-device-pixel rectangle the toolkit puts
/// round the focused item's label, over the selection band.
fn dotted_focus_rect(c: &dyn ControlCanvas, r: &Rect, color: &D2D1_COLOR_F) {
    let t = device_pixel(c);
    dotted_h(c, r.left, r.right, r.top, color);
    dotted_h(c, r.left, r.right, r.bottom - t, color);
    dotted_v(c, r.left, r.top, r.bottom, color);
    dotted_v(c, r.right - t, r.top, r.bottom, color);
}

/// The expand/collapse glyph — the theme's own, falling back to the classic
/// `⊞` / `⊟` box.
///
/// ## Themed
///
/// `TVP_GLYPH`, `GLPS_CLOSED` for a plus and `GLPS_OPENED` for a minus. The
/// sheet's glyph is a `#919191` outline with `#BABBBC` corners round a four-band
/// vertical gradient (`#FCFCFC` → `#FAFBFB` → `#EDEDEC` → `#E3E3E3`) and a
/// `#4B63A7` sign — five colours, none of them a `GetSysColor` index, which is
/// why this could not be reproduced before and needs no colour written down now.
///
/// The part is **TrueSize**: given a larger rectangle it draws its own 9×9 (at
/// 96 DPI) centred rather than stretching. That is exactly [`GLYPH_BOX`], so the
/// box this family already reserved is the box the theme wants — the geometry
/// was right before the rendering was, and neither moves.
///
/// `ground` is the tree's own ground, and it does double duty: the part is
/// partially transparent (its corners blend), so it is what the corners blend
/// into, and the blit is opaque over the whole box, so it is also what cleanly
/// interrupts the dotted connecting lines the glyph sits on top of — which is
/// what the classic branch's `fill_rect` does by hand just below.
///
/// **Not** `TVP_HOTGLYPH`: it is undefined in this theme and renders the
/// fallback white box. The port has no per-row hover in its model either, so
/// there is nothing to lose.
///
/// ## Classic
///
/// A `ControlDark` square over a `Window` field, with a `WindowText` minus bar
/// and — when the node is collapsed — the vertical bar that turns it into a
/// plus. Both bars are one device pixel and inset two device pixels from the
/// box, as the toolkit's own glyph is, so the mark stays a mark and never
/// becomes a slab at 175 %.
fn draw_expand_glyph(c: &dyn ControlCanvas, r: &Rect, expanded: bool, ground: &D2D1_COLOR_F) {
    let glyph_state = if expanded { part::GLPS_OPENED } else { part::GLPS_CLOSED };
    if c.draw_theme_part(theme::class::TREEVIEW, part::TVP_GLYPH, glyph_state, *r, *ground) {
        return;
    }
    let colors = c.visuals().colors;
    let t = device_pixel(c);
    c.fill_rect(r, &colors.window);
    c.stroke_rect(r, &colors.control_dark);
    let inset = 2.0 * t;
    let mid_x = (r.left + r.right) / 2.0;
    let mid_y = (r.top + r.bottom) / 2.0;
    c.fill_rect(
        &Rect::new(r.left + inset, mid_y - t / 2.0, r.right - inset, mid_y + t / 2.0),
        &colors.window_text,
    );
    if !expanded {
        c.fill_rect(
            &Rect::new(mid_x - t / 2.0, r.top + inset, mid_x + t / 2.0, r.bottom - inset),
            &colors.window_text,
        );
    }
}

/// A node/row check box: the classic sunken well — `DrawEdge(EDGE_SUNKEN)` over
/// a `Window` field — with a tick in it when set.
fn draw_check_box(c: &dyn ControlCanvas, r: &Rect, checked: bool, enabled: bool) {
    let colors = c.visuals().colors;
    let inner = c.draw_edge(r, Border3DStyle::Sunken, Border3DSide::ALL);
    c.fill_rect(&inner, &if enabled { colors.window } else { colors.control });
    if checked {
        let ink = if enabled { colors.window_text } else { colors.gray_text };
        draw_check_mark(c, &inner, &ink);
    }
}

/// The tick itself: a short fall meeting a long rise, drawn one device-pixel
/// column at a time.
///
/// A stroked diagonal is not available — the canvas fills rectangles — and two
/// axis-aligned bars read as an arrow rather than a check. Stepping a column at
/// a time is what the toolkit's own bitmap does, and it stays crisp at any DPI.
fn draw_check_mark(c: &dyn ControlCanvas, r: &Rect, color: &D2D1_COLOR_F) {
    let s = c.scale().max(0.01);
    let t = 1.0 / s;
    let w = r.right - r.left;
    let h = r.bottom - r.top;
    // The polyline, in the box's own space.
    let (x0, x1, x2) = (r.left + w * 0.18, r.left + w * 0.42, r.left + w * 0.82);
    let (y0, y1, y2) = (r.top + h * 0.46, r.top + h * 0.74, r.top + h * 0.22);
    let end = (x2 * s).round() as i64;
    let mut i = (x0 * s).round() as i64;
    while i < end {
        let x = i as f32 / s;
        let y = if x < x1 {
            y0 + (y1 - y0) * ((x - x0) / (x1 - x0).max(t))
        } else {
            y1 + (y2 - y1) * ((x - x1) / (x2 - x1).max(t))
        };
        c.fill_rect(&Rect::new(x, y - t, x + t, y + 2.0 * t), color);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Defaults, asserted against the reflection catalogue ─────────────

    #[test]
    fn tree_view_defaults_match_the_catalogue() {
        let t = TreeView::new();
        assert!(t.show_lines && t.show_root_lines && t.show_plus_minus);
        assert!(t.scrollable);
        assert!(t.hide_selection, "TreeView.HideSelection defaults TRUE");
        assert!(!t.check_boxes && !t.full_row_select && !t.label_edit && !t.sorted);
        assert_eq!(t.path_separator, "\\");
        assert_eq!(t.border_style, BorderStyle::Fixed3D);
        assert_eq!(t.image_index, -1);
        assert_eq!(t.selected_image_index, -1);
        assert_eq!(t.draw_mode, TreeViewDrawMode::Normal);
        // Base-carried, reached through Deref:
        assert!(t.enabled && t.visible);
    }

    #[test]
    fn list_view_defaults_match_the_catalogue() {
        let l = ListView::new();
        assert_eq!(l.view, View::LargeIcon);
        assert!(l.multi_select, "ListView.MultiSelect defaults TRUE");
        assert!(!l.hide_selection, "ListView.HideSelection defaults FALSE");
        assert!(l.auto_arrange && l.label_wrap && l.scrollable && l.show_groups);
        assert!(!l.grid_lines && !l.check_boxes && !l.full_row_select);
        assert_eq!(l.header_style, ColumnHeaderStyle::Clickable);
        assert_eq!(l.border_style, BorderStyle::Fixed3D);
        assert_eq!(l.sorting, SortOrder::None);
        assert_eq!(l.alignment, ListViewAlignment::Top);
        assert_eq!(l.activation, ItemActivation::Standard);
        assert_eq!(l.virtual_list_size, 0);
        assert!(!l.virtual_mode && !l.owner_draw);
    }

    #[test]
    fn column_header_default_width_is_sixty() {
        assert_eq!(ColumnHeader::default().width, 60);
    }

    /// `ColumnHeader.TextAlign` defaults to `Left`, which is what the SHARED
    /// `HorizontalAlignment::default()` supplies — asserted rather than assumed,
    /// so moving the type into `enums.rs` cannot have changed this default.
    #[test]
    fn column_header_text_align_defaults_to_left() {
        assert_eq!(ColumnHeader::default().text_align, HorizontalAlignment::Left);
        assert_eq!(HorizontalAlignment::default(), HorizontalAlignment::Left);
        assert_eq!(ColumnHeader::new("Nom", 100).text_align, HorizontalAlignment::Left);
    }

    /// The toolkit cannot align the FIRST Details column: `TextAlign` is forced
    /// to `Left` there. A later reader must not "fix" this into honouring it.
    #[test]
    fn the_first_details_column_ignores_its_declared_alignment() {
        assert_eq!(
            details_column_align(0, HorizontalAlignment::Right),
            HorizontalAlignment::Left,
            "column 0 is forced Left by the toolkit"
        );
        // Every later column honours what it declares.
        assert_eq!(details_column_align(1, HorizontalAlignment::Right), HorizontalAlignment::Right);
        assert_eq!(details_column_align(2, HorizontalAlignment::Center), HorizontalAlignment::Center);
    }

    // ── TreeView: visible-row flattening ────────────────────────────────

    /// The tree from the reference sheet: Instance › Kubuno (expanded) with
    /// Support N1 / Équipe support, then a collapsed « Invités » sibling.
    fn sample_tree() -> Vec<TreeNode> {
        vec![TreeNode::new("Instance")
            .expanded()
            .child(
                TreeNode::new("Kubuno")
                    .expanded()
                    .child(TreeNode::new("Support N1"))
                    .child(TreeNode::new("Équipe support")),
            )
            .child(
                // Collapsed: its child must NOT appear in the visible rows.
                TreeNode::new("Invités").child(TreeNode::new("Anonyme")),
            )]
    }

    #[test]
    fn visible_rows_respect_mixed_expand_state() {
        let tree = sample_tree();
        let rows = visible_rows(&tree);
        let seen: Vec<(&str, usize)> = rows
            .iter()
            .map(|r| (TreeView::node_at(&tree, &r.path).unwrap().text.as_str(), r.depth))
            .collect();
        assert_eq!(
            seen,
            vec![
                ("Instance", 0),
                ("Kubuno", 1),
                ("Support N1", 2),
                ("Équipe support", 2),
                ("Invités", 1),
            ],
            "the collapsed « Invités » must hide « Anonyme »"
        );
    }

    #[test]
    fn collapsing_a_node_drops_its_subtree() {
        let mut tree = sample_tree();
        // Collapse « Kubuno » (path [0,0]).
        TreeView::node_at_mut(&mut tree, &[0, 0]).unwrap().expanded = false;
        let rows = visible_rows(&tree);
        assert_eq!(rows.len(), 3, "Instance, Kubuno, Invités remain");
        assert!(!rows.iter().any(|r| r.path == vec![0, 0, 0]));
    }

    // ── TreeView: FullPath composition ──────────────────────────────────

    #[test]
    fn full_path_uses_the_separator_and_includes_the_root() {
        let mut t = TreeView::new();
        t.nodes = sample_tree();
        assert_eq!(
            t.full_path(&[0, 0, 1]).as_deref(),
            Some("Instance\\Kubuno\\Équipe support")
        );
        // A custom separator, as a caller might set for a URL-like path.
        t.path_separator = "/".to_string();
        assert_eq!(t.full_path(&[0, 0]).as_deref(), Some("Instance/Kubuno"));
        // Invalid path → None, never a panic.
        assert_eq!(t.full_path(&[9]), None);
        assert_eq!(t.full_path(&[]), None);
    }

    // ── TreeView: checkbox does NOT cascade (verified toolkit behaviour) ─

    #[test]
    fn setting_a_node_check_does_not_cascade_to_children() {
        let mut t = TreeView::new();
        t.nodes = sample_tree();
        t.set_node_checked(&[0, 0], true); // check « Kubuno »
        let kubuno = TreeView::node_at(&t.nodes, &[0, 0]).unwrap();
        assert!(kubuno.checked);
        // Children stay unchecked: WinForms leaves cascading to AfterCheck.
        assert!(!kubuno.children[0].checked);
        assert!(!kubuno.children[1].checked);
        // Parent stays unchecked too.
        assert!(!TreeView::node_at(&t.nodes, &[0]).unwrap().checked);
    }

    // ── TreeView: what decides where a connecting line stops ────────────

    /// A branch's vertical line runs through a row only while the level still
    /// has siblings below it. Get this wrong and every branch trails a line off
    /// its own bottom.
    #[test]
    fn a_following_sibling_is_what_keeps_a_line_running() {
        let tree = sample_tree();
        // « Instance » is the only root: nothing follows it.
        assert!(!TreeView::has_following_sibling(&tree, &[0]));
        // « Kubuno » is followed by « Invités »; « Invités » is last.
        assert!(TreeView::has_following_sibling(&tree, &[0, 0]));
        assert!(!TreeView::has_following_sibling(&tree, &[0, 1]));
        // « Support N1 » is followed by « Équipe support ».
        assert!(TreeView::has_following_sibling(&tree, &[0, 0, 0]));
        assert!(!TreeView::has_following_sibling(&tree, &[0, 0, 1]));
        // An empty or invalid path answers false rather than panicking.
        assert!(!TreeView::has_following_sibling(&tree, &[]));
        assert!(!TreeView::has_following_sibling(&tree, &[9, 9]));
    }

    // ── The three-way selection rule ────────────────────────────────────

    /// Focused → the system blue; unfocused → grey, or nothing at all when
    /// `HideSelection` is set. Collapsing this to two cases is what makes a port
    /// paint a blue band on a control that does not have the focus.
    #[test]
    fn the_selection_colours_follow_focus_and_hide_selection() {
        let colors = fake_colors();
        // Focused: Highlight / HighlightText, whatever HideSelection says.
        assert_eq!(
            selection_paint(&colors, true, true),
            Some((colors.highlight, colors.highlight_text))
        );
        assert_eq!(
            selection_paint(&colors, true, false),
            Some((colors.highlight, colors.highlight_text))
        );
        // Unfocused, HideSelection (the TreeView default): nothing is painted.
        assert_eq!(selection_paint(&colors, false, true), None);
        // Unfocused, not hidden (the ListView default): the grey band.
        assert_eq!(
            selection_paint(&colors, false, false),
            Some((colors.control, colors.window_text))
        );
    }

    /// Four distinguishable greys, so a mixed-up field shows as a failure rather
    /// than as two equal colours.
    fn fake_colors() -> SystemColors {
        let g = |v: f32| D2D1_COLOR_F { r: v, g: v, b: v, a: 1.0 };
        SystemColors {
            control:             g(0.75),
            control_text:        g(0.01),
            control_dark:        g(0.5),
            control_dark_dark:   g(0.25),
            control_light:       g(0.85),
            control_light_light: g(1.0),
            window:              g(0.99),
            window_text:         g(0.0),
            window_frame:        g(0.1),
            highlight:           g(0.2),
            highlight_text:      g(0.98),
            gray_text:           g(0.4),
            inactive_border:     g(0.6),
            app_workspace:       g(0.35),
            info_background:     g(0.95),
            info_text:           g(0.05),
            menu_bar:            g(0.9),
            menu_text:           g(0.02),
            button_shadow:       g(0.51),
            button_highlight:    g(0.97),
            hot_track:           g(0.3),
        }
    }

    #[test]
    fn sort_orders_nodes_by_text_recursively() {
        let mut t = TreeView::new();
        t.nodes = vec![TreeNode::new("B")
            .child(TreeNode::new("z"))
            .child(TreeNode::new("a"))];
        t.sort();
        assert_eq!(t.nodes[0].children[0].text, "a");
        assert_eq!(t.nodes[0].children[1].text, "z");
    }

    // ── ListView: Details geometry ──────────────────────────────────────

    fn details_view() -> (ListView, DetailsGeometry) {
        let mut l = ListView::new();
        l.view = View::Details;
        l.columns = vec![
            ColumnHeader::new("Nom", 100),
            ColumnHeader::new("Rôle", 60),
            ColumnHeader::new("Quota", 80),
        ];
        l.items = vec![
            ListViewItem::new("Admin").with_sub("admin").with_sub("10 Go"),
            ListViewItem::new("Alice").with_sub("user").with_sub("5 Go"),
            ListViewItem::new("Bob").with_sub("user").with_sub("5 Go"),
        ];
        let geo = l.details_geometry(Rect::new(0.0, 0.0, 300.0, 200.0));
        (l, geo)
    }

    #[test]
    fn column_x_offsets_accumulate_widths() {
        let (l, geo) = details_view();
        // content.left = 1.0 (Fixed3D inset); widths 100/60/80.
        assert_eq!(geo.column_x_offsets(&l.columns), vec![1.0, 101.0, 161.0, 241.0]);
    }

    #[test]
    fn row_rect_sits_below_the_header() {
        let (_l, geo) = details_view();
        // header 23 + inset 1 → first row top = 24.
        let r0 = geo.row_rect(0);
        assert_eq!(r0.top, 1.0 + DETAILS_HEADER_HEIGHT);
        assert_eq!(r0.bottom, 1.0 + DETAILS_HEADER_HEIGHT + DETAILS_ROW_HEIGHT);
    }

    #[test]
    fn hit_test_maps_a_point_to_item_and_sub_item() {
        let (l, geo) = details_view();
        let body_top = geo.content.top + geo.header_height;
        // A point in row 1, inside the « Rôle » column (x in 101..161).
        let hit = geo.hit_test(&l.columns, l.item_count(), 130.0, body_top + DETAILS_ROW_HEIGHT + 5.0);
        assert_eq!(hit, Some((1, 1)));
        // First column of the first row.
        assert_eq!(geo.hit_test(&l.columns, l.item_count(), 20.0, body_top + 2.0), Some((0, 0)));
    }

    #[test]
    fn hit_test_rejects_the_header_and_the_empty_tail() {
        let (l, geo) = details_view();
        // Inside the header band → None.
        assert_eq!(geo.hit_test(&l.columns, l.item_count(), 20.0, geo.content.top + 2.0), None);
        // Below the last row → None (only 3 items).
        let far = geo.content.top + geo.header_height + 10.0 * DETAILS_ROW_HEIGHT;
        assert_eq!(geo.hit_test(&l.columns, l.item_count(), 20.0, far), None);
    }

    #[test]
    fn cell_maps_column_zero_to_text_then_sub_items() {
        let item = ListViewItem::new("Admin").with_sub("admin").with_sub("10 Go");
        assert_eq!(item.cell(0), "Admin");
        assert_eq!(item.cell(1), "admin");
        assert_eq!(item.cell(2), "10 Go");
        assert_eq!(item.cell(3), "", "a missing column reads empty, never panics");
    }

    // ── ListView: selection under MultiSelect ───────────────────────────

    #[test]
    fn multi_select_off_collapses_to_a_single_selection() {
        let (mut l, _) = details_view();
        l.multi_select = false;
        l.set_selected(0, true);
        l.set_selected(2, true); // must clear item 0
        assert_eq!(l.selected_indices(), vec![2]);
    }

    #[test]
    fn multi_select_on_accumulates_selections() {
        let (mut l, _) = details_view();
        assert!(l.multi_select);
        l.set_selected(0, true);
        l.set_selected(2, true);
        assert_eq!(l.selected_indices(), vec![0, 2]);
        l.set_selected(0, false);
        assert_eq!(l.selected_indices(), vec![2]);
    }

    #[test]
    fn checked_indices_are_derived_from_items() {
        let (mut l, _) = details_view();
        l.items[1].checked = true;
        assert_eq!(l.checked_indices(), vec![1]);
    }

    /// `SelectedItems`/`CheckedItems` are accessors over the same flags the
    /// index readings use, so the two can never disagree — the reason neither is
    /// stored as a second field.
    #[test]
    fn item_and_index_readings_of_a_selection_agree() {
        let (mut l, _) = details_view();
        l.set_selected(0, true);
        l.set_selected(2, true);
        l.items[1].checked = true;

        let sel: Vec<&str> = l.selected_items().map(|it| it.text.as_str()).collect();
        assert_eq!(sel, vec!["Admin", "Bob"]);
        // Same fact, read the other way.
        let by_index: Vec<&str> = l
            .selected_indices()
            .iter()
            .map(|&i| l.items[i].text.as_str())
            .collect();
        assert_eq!(sel, by_index);

        let checked: Vec<&str> = l.checked_items().map(|it| it.text.as_str()).collect();
        assert_eq!(checked, vec!["Alice"]);
        assert_eq!(checked.len(), l.checked_indices().len());
    }

    // ── ListView: virtual mode and List tiling ──────────────────────────

    #[test]
    fn virtual_mode_counts_by_virtual_list_size() {
        let mut l = ListView::new();
        l.virtual_mode = true;
        l.virtual_list_size = 5000;
        assert_eq!(l.item_count(), 5000, "no items fetched — count is the declared size");
    }

    #[test]
    fn list_view_tiling_wraps_into_columns() {
        // An area 3 rows tall (row height 21 → 63px fits 3 rows) with 5 items:
        // items 0,1,2 in column 0; items 3,4 in column 1.
        let area = Rect::new(0.0, 0.0, 300.0, 63.0);
        let rects = list_column_layout(5, LIST_COLUMN_WIDTH, DETAILS_ROW_HEIGHT, area);
        assert_eq!(rects[0].left, 0.0);
        assert_eq!(rects[2].left, 0.0);
        assert_eq!(rects[3].left, LIST_COLUMN_WIDTH, "item 3 wraps to the next column");
        assert_eq!(rects[3].top, 0.0);
        assert_eq!(rects[4].top, DETAILS_ROW_HEIGHT);
    }

    #[test]
    fn type_names_are_stable() {
        assert_eq!(TreeView::new().type_name(), "TreeView");
        assert_eq!(ListView::new().type_name(), "ListView");
    }

    // ── The themed parts, pinned to the pixels they were chosen by ──────
    //
    // Every triple this family adopted was picked by rendering it and sampling
    // it against `C:\kubuno-build\winforms-ref\shots\08-views.png`, never by
    // reading its name — `HP_HEADERITEMLEFT` and `TVP_HOTGLYPH` both sound
    // exactly right and are both undefined. These tests pin the result, so a
    // Windows update that moves a part is a failing test rather than a slow
    // drift, and they are the reason no colour above is written down.
    //
    // They render through GDI and stop short of the Direct2D upload, so they
    // need no device, no window and no swap chain. On a machine with visual
    // styles off there is no themed pixel to assert and every one of them is a
    // documented skip — the classic branch is the whole rendering there.

    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, SelectObject,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows::Win32::UI::Controls::{
        CloseThemeData, DrawThemeBackground, IsAppThemed, IsThemeActive, IsThemePartDefined,
    };
    use windows::Win32::UI::HiDpi::OpenThemeDataForDpi;

    /// One rendered part as `w * h` BGRA words, or `None` when visual styles are
    /// unavailable — which is a **skip**, not a failure.
    ///
    /// The pre-fill is `COLOR_BTNFACE` (`#F0F0F0`), the ground the reference
    /// sheet has behind both of these controls, so a sampled pixel compares with
    /// the sheet directly — and a TrueSize part that does not cover its
    /// rectangle leaves it visible, which is how the glyph's sizing is measured.
    fn sample(class: PCWSTR, part: i32, state: i32, w: i32, h: i32) -> Option<Vec<u32>> {
        if !unsafe { IsThemeActive().as_bool() && IsAppThemed().as_bool() } {
            return None;
        }
        let theme = unsafe { OpenThemeDataForDpi(None, class, 96) };
        if theme.is_invalid() {
            return None;
        }
        let out = unsafe {
            let dc = CreateCompatibleDC(None);
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    // Negative: top-down, so row 0 is the top one.
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
            let bmp = CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
                .ok()
                .filter(|b| !b.is_invalid() && !bits.is_null());
            let out = bmp.map(|bmp| {
                let prev = SelectObject(dc, bmp.into());
                let px = std::slice::from_raw_parts_mut(bits.cast::<u32>(), (w * h) as usize);
                px.fill(0xFFF0_F0F0);
                let r = RECT { left: 0, top: 0, right: w, bottom: h };
                let drawn = DrawThemeBackground(theme, dc, part, state, &r, None).is_ok();
                // GDI batches per thread: without this the bits can be read
                // before the theme has drawn them, intermittently.
                let _ = GdiFlush();
                let out = drawn.then(|| px.to_vec());
                SelectObject(dc, prev);
                let _ = DeleteObject(bmp.into());
                out
            });
            let _ = DeleteDC(dc);
            out.flatten()
        };
        let _ = unsafe { CloseThemeData(theme) };
        out
    }

    /// `#RRGGBB` for a BGRA word, so a failure prints the colour a human can
    /// hold against the reference sheet rather than a decimal.
    fn hex(px: u32) -> String {
        format!("#{:02X}{:02X}{:02X}", (px >> 16) & 0xFF, (px >> 8) & 0xFF, px & 0xFF)
    }

    /// Whether a part exists in the current theme at all — the question that
    /// `DrawThemeBackground`'s return value does **not** answer.
    fn defined(class: PCWSTR, part: i32) -> Option<bool> {
        if !unsafe { IsThemeActive().as_bool() && IsAppThemed().as_bool() } {
            return None;
        }
        let theme = unsafe { OpenThemeDataForDpi(None, class, 96) };
        if theme.is_invalid() {
            return None;
        }
        let d = unsafe { IsThemePartDefined(theme, part, 0) }.as_bool();
        let _ = unsafe { CloseThemeData(theme) };
        Some(d)
    }

    /// The finding that closes the header gap: the sheet's Details header is a
    /// `#FFFFFF` face with an `#E5E5E5` rule, and neither is a `SystemColors`
    /// entry. `HP_HEADERITEM` produces both, and produces the rule on its own
    /// **right edge** — which is why the port draws no separator itself.
    #[test]
    fn the_header_item_is_the_reference_face_and_rule() {
        let (w, h) = (20usize, 39usize);
        let Some(px) =
            sample(w!("HEADER"), part::HP_HEADERITEM, part::HIS_NORMAL, w as i32, h as i32)
        else {
            eprintln!("[views] visual styles unavailable — themed header not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(hex(px[mid * w]), "#FFFFFF", "the header face");
        assert_eq!(hex(px[mid * w + w / 2]), "#FFFFFF", "the face, mid-item");
        assert_eq!(hex(px[mid * w + w - 1]), "#E5E5E5", "the rule, on the item's last column");
        // The rule runs the item's whole height and there is no bottom edge: the
        // sheet shows no line under the header band, only the rows' own grid.
        assert_eq!(hex(px[w - 1]), "#E5E5E5", "the rule reaches the top");
        assert_eq!(hex(px[(h - 1) * w + w - 1]), "#E5E5E5", "and the bottom");
        assert_eq!(hex(px[(h - 1) * w + w / 2]), "#FFFFFF", "no bottom rule under the band");
    }

    /// The two states nothing can reach honestly yet are still measured, so the
    /// day a `ColumnHeader` carries a hover flag the colours are already known
    /// — and so `HIS_NORMAL` is demonstrably a state and not a constant.
    #[test]
    fn a_hot_or_pressed_header_item_has_its_own_face() {
        let (w, h) = (20usize, 39usize);
        let (Some(hot), Some(pressed)) = (
            sample(w!("HEADER"), part::HP_HEADERITEM, part::HIS_HOT, w as i32, h as i32),
            sample(w!("HEADER"), part::HP_HEADERITEM, part::HIS_PRESSED, w as i32, h as i32),
        ) else {
            eprintln!("[views] visual styles unavailable — header states not asserted");
            return;
        };
        let mid = h / 2 * w + w / 2;
        assert_eq!(hex(hot[mid]), "#D9EBF9", "HIS_HOT");
        assert_eq!(hex(pressed[mid]), "#BCDCF4", "HIS_PRESSED");
    }

    /// The sort arrow really points the way its state names, and really is
    /// TrueSize — it centres a small triangle in a big box rather than
    /// stretching, which is why [`SORT_ARROW_BOX`] only has to be big enough.
    #[test]
    fn the_sort_arrow_points_the_way_its_state_says() {
        let (w, h) = (26usize, 12usize);
        let (Some(up), Some(down)) = (
            sample(w!("HEADER"), part::HP_HEADERSORTARROW, part::HSAS_SORTEDUP, w as i32, h as i32),
            sample(
                w!("HEADER"),
                part::HP_HEADERSORTARROW,
                part::HSAS_SORTEDDOWN,
                w as i32,
                h as i32,
            ),
        ) else {
            eprintln!("[views] visual styles unavailable — sort arrow not asserted");
            return;
        };
        // Untouched pixels keep the pre-fill, so counting them measures the ink.
        let ink =
            |px: &[u32], row: usize| (0..w).filter(|&x| px[row * w + x] != 0xFFF0_F0F0).count();
        // The rows the triangle actually occupies, found rather than assumed —
        // the part centres itself, so where it lands depends on the box.
        let span = |px: &[u32]| {
            let rows: Vec<usize> = (0..h).filter(|&y| ink(px, y) > 0).collect();
            (*rows.first().unwrap(), *rows.last().unwrap())
        };
        let (up_top, up_bottom) = span(&up);
        let (down_top, down_bottom) = span(&down);
        // The triangle is centred in the box, not stretched over it: the outer
        // rows are pre-fill on both.
        assert_eq!(ink(&up, 0), 0, "TrueSize: the top row is untouched");
        assert_eq!(ink(&up, h - 1), 0, "TrueSize: the bottom row is untouched");
        assert!(up_bottom - up_top < h / 2, "TrueSize: it uses a fraction of the box");
        // Up widens downwards; down narrows downwards. That is the whole
        // difference between the two states, and it is a shape, not a colour.
        assert!(
            ink(&up, up_top) < ink(&up, up_bottom),
            "HSAS_SORTEDUP widens towards the bottom"
        );
        assert!(
            ink(&down, down_top) > ink(&down, down_bottom),
            "HSAS_SORTEDDOWN narrows towards the bottom"
        );
    }

    /// The third gap this family could not close: the sheet's expand glyph is a
    /// `#919191` outline with `#BABBBC` corners, a pale vertical gradient and a
    /// `#4B63A7` sign. Five colours, no `SystemColors` entry among them.
    ///
    /// `GLPS_CLOSED` is the plus and `GLPS_OPENED` the minus — asserted as the
    /// vertical bar's presence, which is a shape rather than a shade and so
    /// survives a theme that repaints the glyph.
    #[test]
    fn the_tree_glyph_is_the_reference_glyph() {
        let n = 9usize;
        let (Some(closed), Some(opened)) = (
            sample(w!("TREEVIEW"), part::TVP_GLYPH, part::GLPS_CLOSED, n as i32, n as i32),
            sample(w!("TREEVIEW"), part::TVP_GLYPH, part::GLPS_OPENED, n as i32, n as i32),
        ) else {
            eprintln!("[views] visual styles unavailable — themed glyph not asserted");
            return;
        };
        let mid = n / 2;
        assert_eq!(hex(closed[mid * n]), "#919191", "the glyph's outline");
        assert_eq!(hex(closed[0]), "#BABBBC", "its softened corner");
        assert_eq!(hex(closed[mid * n + mid]), "#4B63A7", "the sign");
        assert_eq!(hex(opened[mid * n]), "#919191", "the outline is state-invariant");
        assert_eq!(hex(opened[mid * n + mid]), "#4B63A7", "so is the sign's colour");
        // The horizontal bar is in both; only the closed glyph has the vertical
        // one that turns a minus into a plus. Two rows above the middle is inside
        // that bar and inside the plain gradient on the opened glyph, so the two
        // differing there IS the plus — asserted as a shape rather than a shade,
        // since the bar's own ink (`#294272`) is the sign blended into the
        // gradient and would move with it.
        assert_ne!(
            hex(closed[2 * n + mid]),
            hex(opened[2 * n + mid]),
            "GLPS_CLOSED has the vertical bar GLPS_OPENED lacks"
        );
        assert_eq!(
            hex(opened[2 * n + mid]),
            hex(opened[2 * n + mid - 2]),
            "GLPS_OPENED is plain gradient there"
        );
    }

    /// `TVP_GLYPH` is TrueSize, and its natural side at 96 DPI is exactly
    /// [`GLYPH_BOX`] — which is why adopting the theme moved no geometry: given
    /// a larger box it centres its 9×9 and leaves the rest as it found it.
    #[test]
    fn the_tree_glyph_draws_its_own_size() {
        let n = 16usize;
        let Some(px) = sample(w!("TREEVIEW"), part::TVP_GLYPH, part::GLPS_OPENED, n as i32, n as i32)
        else {
            eprintln!("[views] visual styles unavailable — glyph sizing not asserted");
            return;
        };
        assert_eq!(px[0], 0xFFF0_F0F0, "a 16 px box keeps its pre-filled corner");
        let drawn: Vec<usize> =
            (0..n).filter(|&x| px[(n / 2) * n + x] != 0xFFF0_F0F0).collect();
        let side = drawn.last().unwrap() - drawn.first().unwrap() + 1;
        assert_eq!(side, GLYPH_BOX as usize, "the natural side is GLYPH_BOX");
    }

    /// The port's own frame, the same `EP_EDITTEXT` line a `TextBox` is framed
    /// with — one flat `#ABADB3` pixel, which is what the sheet shows round both
    /// views and which no `GetSysColor` index carries.
    #[test]
    fn the_view_frame_is_the_reference_border() {
        let (w, h) = (40usize, 30usize);
        let (Some(normal), Some(disabled)) = (
            sample(w!("EDIT"), EP_EDITTEXT, ETS_NORMAL, w as i32, h as i32),
            sample(w!("EDIT"), EP_EDITTEXT, ETS_DISABLED, w as i32, h as i32),
        ) else {
            eprintln!("[views] visual styles unavailable — themed frame not asserted");
            return;
        };
        let mid = h / 2;
        assert_eq!(hex(normal[mid * w]), "#ABADB3", "the frame");
        assert_eq!(hex(disabled[mid * w]), "#ABADB3", "the frame is state-invariant");
        // The FILL is not, which is exactly why `paint_border` puts the pad ring
        // back in the control's own ground instead of trusting the part's.
        assert_ne!(normal[mid * w + 1], disabled[mid * w + 1], "the fill is not");
    }

    /// The measurement that kept the selection bands on `SystemColors`.
    ///
    /// `LVP_LISTITEM` and `TVP_TREEITEM` are the parts this family was expected
    /// to adopt and they are **not in this theme at all** — nor is
    /// `TVP_HOTGLYPH`, nor `HP_HEADERITEMLEFT`/`RIGHT`. If a future Windows
    /// defines any of them this test fails, which is the point: the decision is
    /// then revisited against the sheet rather than left to rot.
    ///
    /// It also pins the trap itself — drawing an undefined part **succeeds** and
    /// paints a white box in a `#828790` frame — because that is the reason the
    /// check cannot be "did `draw_theme_part` return true".
    #[test]
    fn the_item_parts_are_not_in_this_theme() {
        use windows::Win32::UI::Controls as sdk;
        let Some(_) = defined(w!("TREEVIEW"), part::TVP_GLYPH) else {
            eprintln!("[views] visual styles unavailable — part table not asserted");
            return;
        };
        // The adopted ones exist…
        assert_eq!(defined(w!("TREEVIEW"), part::TVP_GLYPH), Some(true), "TVP_GLYPH");
        assert_eq!(defined(w!("HEADER"), part::HP_HEADERITEM), Some(true), "HP_HEADERITEM");
        assert_eq!(defined(w!("HEADER"), part::HP_HEADERSORTARROW), Some(true), "HP_HEADERSORTARROW");
        assert_eq!(defined(w!("EDIT"), EP_EDITTEXT), Some(true), "EP_EDITTEXT");
        // …and every part whose name sounds right does not.
        for (label, class, id) in [
            ("LVP_LISTITEM", w!("LISTVIEW"), sdk::LVP_LISTITEM.0),
            ("TVP_TREEITEM", w!("TREEVIEW"), sdk::TVP_TREEITEM.0),
            ("TVP_HOTGLYPH", w!("TREEVIEW"), sdk::TVP_HOTGLYPH.0),
            ("HP_HEADERITEMLEFT", w!("HEADER"), sdk::HP_HEADERITEMLEFT.0),
            ("HP_HEADERITEMRIGHT", w!("HEADER"), sdk::HP_HEADERITEMRIGHT.0),
        ] {
            assert_eq!(defined(class, id), Some(false), "{label} is defined after all — re-measure");
        }
        // The trap: it draws anyway, and it draws the wrong thing.
        let (w, h) = (40usize, 21usize);
        let Some(px) = sample(w!("LISTVIEW"), sdk::LVP_LISTITEM.0, sdk::LISS_SELECTEDNOTFOCUS.0, w as i32, h as i32)
        else {
            panic!("an undefined part still draws — that is the whole finding");
        };
        assert_eq!(hex(px[0]), "#828790", "the fallback's frame, not a selection");
        assert_eq!(hex(px[(h / 2) * w + w / 2]), "#FFFFFF", "the fallback's white box");
        // And what the sheet actually shows there is a system colour, exactly.
        let colors = fake_colors();
        assert_eq!(
            selection_paint(&colors, false, false).map(|(b, _)| b.r),
            Some(colors.control.r),
            "the unfocused band is Control — #F0F0F0 on the sheet"
        );
    }
}

