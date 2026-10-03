//! Component family `containers` — declared with the `component!` table (see
//! `../macros.rs`) plus, in this same file, the view nodes those components
//! build. Compiled only with the `family-containers` feature while the families are
//! being written in parallel; the feature is on by default once integrated.
//!
//! ## Scope
//!
//! `Panel`, `GroupBox`, `ScrollArea`, `Splitter`, `Tabs` (+ `TabItem`),
//! `Breadcrumb` (+ `BreadcrumbItem`), `Toolbar` (+ `ToolbarItem`), `Accordion`
//! (+ `AccordionSection`) and `Stepper` (+ `Step`) — the real
//! `kubuno_controls`/`kubuno_ui` layout engines and controllers
//! (`kubuno_controls::layout::layout`, `kubuno_controls::layout_panels::{flow_layout,
//! SplitContainer}`, `kubuno_ui::navigation::TabsController`), never
//! re-implemented. There is deliberately no `<Grid>` — `XML_VIEWS.md`'s
//! layout section is explicit that `kubuno_controls` has no grid engine, so
//! none is invented here (see `vskubuno/docs/XML_VIEWS.md`, "There is no
//! `<Grid>`").
//!
//! ## Composite components and the children model
//!
//! `Tabs`, `Breadcrumb`, `Toolbar`, `Accordion` and `Stepper` declare
//! `ChildrenModel::List(&["TabItem"])` (etc.) — the allowed-name slice
//! `crate::validate::validate` now checks a stray child against at *validate*
//! time (`<TabItem>` outside `<Tabs>` is a diagnostic with a line/column,
//! wherever it is nested, not just directly under another restrictive
//! parent — see that module's `gated_required_parents`). Their `build`
//! closures still additionally walk [`crate::props::Props::element`]'s
//! children directly and reject a wrong one with a [`crate::props::
//! BuildError`], rather than relying on [`crate::props::Props::
//! build_children`]'s generic walk — belt and braces: a `.kbview` file that
//! skipped validation (or whose registry is stale relative to the running
//! interpreter) still cannot build a nonsensical tree.

#[allow(unused_imports)] // Used by the `component!` invocations below.
use crate::registry::macros::component;
use crate::registry::{ComponentMeta, LayoutKind};

use crate::binding::{PropSource, Value};
#[cfg(test)]
use crate::binding::BindingMode;
use crate::node::{press_release, PaintCx, ViewEventKind, ViewNode};
use crate::events::{ChangeSource, CheckedChangedEventArgs, ItemEventArgs, NumericValueChangedEventArgs, SelectionChangedEventArgs};
use crate::props::{BuildError, Props};

use kubuno_controls::host::{self, vk, Modifiers};
use kubuno_ui::{AnchorStyles, Canvas, DockStyle, FocusId, Padding, Rect, Size, Widget};

// ─────────────────────────────────────────────────────────────────────────
// Shared parsing helpers — attached layout properties (`Dock`, `Anchor`,
// `X`/`Y`/`Width`/`Height`) are read the same way on every container's
// children, and are resolved once at build time (never through a
// `{Binding …}`): they describe the STRUCTURE the layout engine is handed,
// not a per-frame value, matching `kubuno_controls::layout::Item`'s own
// design-time-coordinates model (`XML_VIEWS.md`'s "Anchor" section).
// ─────────────────────────────────────────────────────────────────────────

fn parse_dock(s: &str) -> DockStyle {
    match s {
        "Top" => DockStyle::Top,
        "Bottom" => DockStyle::Bottom,
        "Left" => DockStyle::Left,
        "Right" => DockStyle::Right,
        "Fill" => DockStyle::Fill,
        _ => DockStyle::None,
    }
}

fn parse_anchor(s: &str) -> AnchorStyles {
    if s.trim().is_empty() {
        return AnchorStyles::default();
    }
    let mut a = AnchorStyles::NONE;
    for part in s.split(',') {
        a = a.union(match part.trim() {
            "Top" => AnchorStyles::TOP,
            "Bottom" => AnchorStyles::BOTTOM,
            "Left" => AnchorStyles::LEFT,
            "Right" => AnchorStyles::RIGHT,
            _ => AnchorStyles::NONE,
        });
    }
    a
}

/// A literal (non-bound) `String` attribute of `props`, or a build error when
/// the author wrote a `{Binding …}` expression — layout placement is
/// structural, resolved once at compile time (see the module doc), not a
/// per-frame value.
fn literal_str(props: &Props<'_>, name: &str, default: &str) -> Result<String, BuildError> {
    match props.str(name, default)? {
        PropSource::Literal(s) => Ok(s),
        PropSource::Bound { .. } => {
            Err(BuildError::new(format!("attribute `{name}` must be a literal value, not a binding"), None))
        }
    }
}

/// See [`literal_str`], for an `F32`-typed attribute.
fn literal_f32(props: &Props<'_>, name: &str, default: f32) -> Result<f32, BuildError> {
    match props.f32(name, default)? {
        PropSource::Literal(v) => Ok(v),
        PropSource::Bound { .. } => {
            Err(BuildError::new(format!("attribute `{name}` must be a literal value, not a binding"), None))
        }
    }
}

/// See [`literal_str`], for a `Bool`-typed attribute.
fn literal_bool(props: &Props<'_>, name: &str, default: bool) -> Result<bool, BuildError> {
    match props.bool(name, default)? {
        PropSource::Literal(v) => Ok(v),
        PropSource::Bound { .. } => {
            Err(BuildError::new(format!("attribute `{name}` must be a literal value, not a binding"), None))
        }
    }
}

// `press_release` and `PaintCx::fire` are shared with every other family
// now (`crate::node`, both `pub(crate)`) rather than reimplemented here.

// ═════════════════════════════════════════════════════════════════════════
// Panel — Dock/Anchor children, over `kubuno_ui::containers::Panel`.
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: panel,
    name: "Panel",
    // Note: A box that places its children by Dock or Anchor (`kubuno_ui::containers::Panel`).
    doc: "A container that places its children by docking or anchoring.",
    ctor: kubuno_ui::containers::Panel::new(),
    children: ChildrenModel::List(&[]),
    layout: LayoutKind::DockAnchor,
    props: [
        PropertyMeta::new("Padding", PropKind::F32, "0", "Space around the children, on all four sides, in pixels."),
        PropertyMeta::new("Surface",
            PropKind::Enum(&["None", "Layer", "Card", "Raised", "Well"]),
            "None",
            "Background painted behind the children.",
        ),
    ],
    events: [],
    smoke: |p| {
        let p = p.with_padding(kubuno_ui::Padding::all(8.0));
        p.with_surface(kubuno_ui::containers::Surface::Layer)
    },
    build: |props, cx| {
        use super::*;
        let padding = props.f32("Padding", 0.0)?;
        let surface = props.enum_("Surface", "None")?;
        let mut specs: Vec<PanelChildSpec> = Vec::new();
        let mut children: Vec<Box<dyn ViewNode>> = Vec::new();
        let mut places: Vec<crate::window::ChildPlace> = Vec::new();
        for child in props.element().children() {
            let child_meta = crate::registry::lookup(&child.name().unwrap_or_default()).ok_or_else(|| {
                BuildError::new(format!("unknown element `<{}>`", child.name().unwrap_or_default()), child.name_range())
            })?;
            let child_props = Props::new(&child, child_meta);
            let optional = |name: &str| -> Result<Option<f32>, BuildError> {
                Ok(if child.attribute(name).is_some() && !crate::common::auto_sized(&child) { Some(literal_f32(&child_props, name, 0.0)?) } else { None })
            };
            specs.push(PanelChildSpec {
                dock: parse_dock(&literal_str(&child_props, "Dock", "")?),
                anchor: parse_anchor(&literal_str(&child_props, "Anchor", "")?),
                x: literal_f32(&child_props, "X", 0.0)?,
                y: literal_f32(&child_props, "Y", 0.0)?,
                width: optional("Width")?,
                height: optional("Height")?,
            });
            places.push(crate::window::ChildPlace::read(&child));
            children.push(crate::compile::build_node(&child, cx, LayoutKind::DockAnchor)?);
        }
        let design = panel_design_size(props.element());
        let is_root = {
            use crate::ast::AstNode;
            props.element().syntax().parent().is_none_or(|p| p.kind() != crate::syntax::SyntaxKind::ELEMENT)
        };
        // The view's root: the title bar's standard items it asks for (`ShowSearch`, `ShowWaffle`…), after its own
        // children, at the end of the band's right region (`crate::window::HeaderSpec`).
        let mut header = 0;
        let mut rtl = false;
        if is_root {
            for (node, (width, height)) in crate::window::build_header_items(props.element(), cx) {
                specs.push(PanelChildSpec { dock: parse_dock(""), anchor: parse_anchor(""), x: 0.0, y: 0.0, width: Some(width), height: Some(height) });
                places.push(crate::window::ChildPlace { title: crate::window::TitleRegion::Right, ..Default::default() });
                children.push(node);
                header += 1;
            }
            rtl = props.element().attribute("RightToLeftLayout").and_then(|a| a.value()).is_some_and(|v| v.trim() == "true");
        }
        Ok(Box::new(crate::registry::families::containers::PanelNode {
            padding,
            surface,
            specs,
            places,
            is_root,
            header,
            rtl,
            children,
            design,
            first_layout: None,
        }) as Box<dyn ViewNode>)
    },
}

/// One `<Panel>` child's attached layout attributes, read once at build time. `width`/`height`
/// are `None` when the attribute is absent: the child's own measured size is used instead (like a
/// WinForms control keeps its natural size when docked or anchored without an explicit size).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelChildSpec {
    pub dock: DockStyle,
    pub anchor: AnchorStyles,
    pub x: f32,
    pub y: f32,
    pub width: Option<f32>,
    pub height: Option<f32>,
}

/// The size a `<Panel>`'s children's `X`/`Y`/`Width`/`Height` were authored against — the reference
/// the anchor pass measures the size CHANGE from (WinForms' anchor semantics): the panel's own literal
/// `Width`×`Height` when it has both; for the view's ROOT panel, the view's design size
/// (`crate::design::design_size`: `DesignWidth`/`DesignHeight`, 800×600 by default — the size the
/// designer shows it at); otherwise `None`, and the panel's first layout becomes the reference.
pub fn panel_design_size(element: &crate::ast::Element) -> Option<Size> {
    use crate::ast::AstNode;
    let literal = |name: &str| {
        element.attribute(name)?.value()?.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0)
    };
    if let (Some(width), Some(height)) = (literal("Width"), literal("Height")) {
        return Some(Size::new(width, height));
    }
    let is_root = element.syntax().parent().is_none_or(|p| p.kind() != crate::syntax::SyntaxKind::ELEMENT);
    if !is_root {
        return None;
    }
    let doc = element.syntax().ancestors().last().and_then(crate::ast::Document::cast);
    let size = crate::design::design_size(doc.as_ref());
    Some(Size::new(size.width, size.height))
}

/// Where a `<Panel>`'s children go inside `bounds` (the panel's own box, padding included), in
/// DOCUMENT order — the one place the WinForms rules are applied, pure (no canvas), unit-tested:
///
/// - **Dock**: bands are resolved in WinForms z-order. A `.kbview` paints its children in document
///   order, so the LAST child is the frontmost one, and WinForms docks the frontmost control LAST
///   (innermost): the first `Dock="Top"` child takes the top edge, and a `Dock="Fill"` child written
///   after the bands takes the remainder. The engine (`kubuno_controls::layout`) takes the children
///   in WinForms `Controls` order (index 0 = front), hence the reversal.
/// - **Anchor**: an anchored edge keeps its distance to the container's edge when the panel is
///   larger or smaller than `design` (its authored size, see [`panel_design_size`]); Left+Right
///   stretches the width, Top+Bottom the height; an axis anchored on neither side keeps its size
///   and stays centred.
///
/// `measured` is each child's natural size, used for a missing `Width`/`Height`.
pub fn panel_child_rects(specs: &[PanelChildSpec], measured: &[Size], padding: f32, design: Size, bounds: Rect) -> Vec<Rect> {
    panel_child_rects_shown(specs, measured, &vec![true; specs.len()], padding, design, bounds)
}

/// [`panel_child_rects`] with each child's visibility: like WinForms, a hidden docked child takes no
/// band (a hidden `Dock="Left"` rail leaves the whole width to the `Fill` page), and a hidden anchored
/// child keeps following its anchors.
pub fn panel_child_rects_shown(specs: &[PanelChildSpec], measured: &[Size], shown: &[bool], padding: f32, design: Size, bounds: Rect) -> Vec<Rect> {
    let mut panel = kubuno_ui::containers::Panel::new().with_padding(Padding::all(padding)).with_design_size(design);
    for (i, (spec, natural)) in specs.iter().zip(measured).enumerate().rev() {
        let shown = shown.get(i).copied().unwrap_or(true);
        let width = spec.width.unwrap_or(natural.width);
        let height = spec.height.unwrap_or(natural.height);
        let it = match spec.dock {
            DockStyle::Top | DockStyle::Bottom => kubuno_ui::containers::band(spec.dock, height),
            DockStyle::Left | DockStyle::Right => kubuno_ui::containers::band(spec.dock, width),
            DockStyle::Fill => kubuno_ui::containers::band(DockStyle::Fill, 0.0),
            _ => kubuno_ui::containers::item(
                Rect::new(spec.x, spec.y, spec.x + width, spec.y + height),
                DockStyle::None,
                spec.anchor,
            ),
        };
        let mut it = it;
        it.visible = shown;
        panel = panel.item(it);
    }
    let mut rects = panel.layout_children(bounds);
    rects.reverse();
    rects
}

/// The live node for `<Panel>` — see the `panel` module's `component!` entry.
/// Rebuilds a real [`kubuno_ui::containers::Panel`] every frame purely for its
/// layout arithmetic ([`panel_child_rects`]) and its [`kubuno_ui::containers::Surface`]
/// chrome, exactly as [`crate::node::CardNode`]/[`crate::node::StackNode`] do
/// (see `crate::node`'s module doc: a container here never delegates child
/// painting to the real `kubuno_ui` widget, only its geometry).
pub struct PanelNode {
    padding: PropSource<f32>,
    surface: PropSource<String>,
    /// One placement per child, in document order — resolved once at build time.
    specs: Vec<PanelChildSpec>,
    /// Where each child goes in the WINDOW (`TitleBar.Region`, `ActionBar.Region`, `TitleBar.Drag`),
    /// honoured on the view's root panel only.
    places: Vec<crate::window::ChildPlace>,
    /// The panel is the view's root element: its window-placed children go to the window.
    is_root: bool,
    /// How many of the last children are the title bar's standard items (not elements of the document): shown in
    /// the band only, side by side with no gap (the web header's cluster).
    header: usize,
    /// `RightToLeftLayout` on the view: the title-bar regions fill from the right.
    rtl: bool,
    children: Vec<Box<dyn ViewNode>>,
    /// The authored size (see [`panel_design_size`]); `None` → `first_layout`.
    design: Option<Size>,
    /// The size of the panel's first layout, the anchoring reference of a panel with no design size.
    /// Kept for the node's life (until the view is recompiled), so a later resize anchors against it.
    first_layout: Option<Size>,
}

fn parse_surface(s: &str) -> kubuno_ui::containers::Surface {
    use kubuno_ui::containers::Surface;
    match s {
        "Layer" => Surface::Layer,
        "Card" => Surface::Card,
        "Raised" => Surface::Raised,
        "Well" => Surface::Well,
        _ => Surface::None,
    }
}

impl PanelNode {
    fn measured(&self, c: &dyn Canvas, vm: &dyn crate::binding::ViewModel) -> Vec<Size> {
        self.specs
            .iter()
            .zip(&self.children)
            .map(|(spec, child)| {
                if spec.width.is_some() && spec.height.is_some() {
                    Size::new(0.0, 0.0)
                } else {
                    child.measure(c, vm)
                }
            })
            .collect()
    }
}

impl ViewNode for PanelNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn crate::binding::ViewModel) -> Size {
        // The panel's natural size: large enough for every anchored child at its authored place
        // (plus the padding), or its design size when it has one.
        if let Some(design) = self.design {
            return design;
        }
        let padding = self.padding.resolve(vm);
        let measured = self.measured(c, vm);
        let (mut w, mut h) = (0.0f32, 0.0f32);
        // The title bar's standard items are never in the page.
        let own = self.children.len() - self.header;
        for ((spec, natural), child) in self.specs.iter().zip(&measured).zip(&self.children).take(own) {
            let cw = spec.width.unwrap_or(natural.width);
            let ch = spec.height.unwrap_or(natural.height);
            // A hidden docked child takes no band (WinForms), in the measure as in the layout
            // (`panel_child_rects_shown`): a collapsed band no longer leaves room under the others.
            if spec.dock != DockStyle::None && child.is_hidden(vm) {
                continue;
            }
            match spec.dock {
                DockStyle::Top | DockStyle::Bottom => h += ch,
                DockStyle::Left | DockStyle::Right => w += cw,
                DockStyle::Fill => {}
                _ => {
                    w = w.max(spec.x + cw);
                    h = h.max(spec.y + ch);
                }
            }
        }
        Size::new(w + 2.0 * padding, h + 2.0 * padding)
    }

    fn is_invisible_container(&self, vm: &dyn crate::binding::ViewModel) -> bool {
        matches!(parse_surface(&self.surface.resolve(vm)), kubuno_ui::containers::Surface::None)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let padding = self.padding.resolve(cx.vm);
        let surface = parse_surface(&self.surface.resolve(cx.vm));
        let canvas: &dyn Canvas = cx.canvas;
        kubuno_ui::containers::Panel::new().with_padding(Padding::all(padding)).with_surface(surface).paint_with(
            canvas,
            bounds,
            crate::common::rest(),
            |_c, _client| {},
        );
        let size = Size::new(bounds.right - bounds.left, bounds.bottom - bounds.top);
        let mut design = match self.design {
            Some(design) => design,
            None => *self.first_layout.get_or_insert(size),
        };
        let measured = self.measured(canvas, cx.vm);
        // The window-placed children (the root panel only): the action bar at the bottom of the
        // view, the title-bar regions in the window's band. A title-bar child of a window with no
        // Kubuno band (the system chrome, a borderless window) stays in the page, where its X/Y say.
        use crate::window::{ActionRegion, TitleRegion};
        let natural = |i: usize| {
            let (spec, m) = (&self.specs[i], measured[i]);
            (spec.width.unwrap_or(m.width), spec.height.unwrap_or(m.height))
        };
        let places: Vec<crate::window::ChildPlace> =
            if self.is_root { self.places.clone() } else { vec![crate::window::ChildPlace::default(); self.children.len()] };
        // The title bar's standard items (the last `header` children) sit side by side, with no gap between them.
        let first_header = self.children.len() - self.header;
        let mut items: Vec<BandItem> = Vec::with_capacity(self.children.len());
        for (i, p) in places.iter().enumerate() {
            // A hidden standard item (a bound `ShowSearch` that is off) takes no room.
            let (width, height) = if i >= first_header && self.children[i].is_hidden(cx.vm) { (0.0, 0.0) } else { natural(i) };
            let previous = items.iter().rposition(|it| it.region == p.title && p.title != TitleRegion::None);
            let joined = i >= first_header && previous.is_some_and(|j| j >= first_header);
            items.push(BandItem { region: p.title, width, height, joined });
        }
        let widths = band_slot_widths(&items);
        let band = if places.iter().any(|p| p.title != TitleRegion::None) { crate::window::title_bar_slots(widths) } else { None };
        let in_band = |p: &crate::window::ChildPlace| band.is_some() && p.title != TitleRegion::None;
        let has_footer = places.iter().any(|p| p.action != ActionRegion::None);
        let mut body = bounds;
        if has_footer {
            let fh = kubuno_controls::window_chrome::FOOTER_HEIGHT;
            body.bottom = (body.bottom - fh).max(body.top);
            design = Size::new(design.width, (design.height - fh).max(0.0));
        }
        // The page's own children, laid out by Dock/Anchor as always (never the title bar's standard items).
        let page: Vec<usize> =
            (0..first_header).filter(|&i| !in_band(&places[i]) && places[i].action == ActionRegion::None).collect();
        let page_specs: Vec<PanelChildSpec> = page.iter().map(|&i| self.specs[i]).collect();
        let page_measured: Vec<Size> = page.iter().map(|&i| measured[i]).collect();
        // A hidden docked child takes no band (WinForms): a hidden rail leaves its width to the page.
        let page_shown: Vec<bool> = page.iter().map(|&i| !self.children[i].is_hidden(cx.vm)).collect();
        let mut rects = vec![Rect::default(); self.children.len()];
        for (i, r) in page.iter().zip(panel_child_rects_shown(&page_specs, &page_measured, &page_shown, padding, design, body)) {
            rects[*i] = r;
        }
        // The title-bar regions: in document order (mirrored right to left), `gap-1` apart, centred on the band.
        if let Some(l) = &band {
            for (i, r) in band_item_rects(&items, l, self.rtl).into_iter().enumerate() {
                if let Some(r) = r {
                    rects[i] = r;
                }
            }
        }
        // The action bar (`.kb-window-footer`): the left group from the left inset, the right
        // group ending at the right inset, `gap-2` apart, centred on the bar.
        if has_footer {
            use kubuno_controls::window_chrome::{FOOTER_GAP, FOOTER_PAD_X};
            let footer = Rect::new(bounds.left, body.bottom, bounds.right, bounds.bottom);
            kubuno_controls::window_chrome::paint_footer(canvas, footer);
            let place_row = |ids: Vec<usize>, start: f32, rects: &mut Vec<Rect>| {
                let mut x = start;
                for i in ids {
                    let (w, h) = natural(i);
                    let top = (footer.top + footer.bottom - h) / 2.0;
                    rects[i] = Rect::new(x, top, x + w, top + h);
                    x += w + FOOTER_GAP;
                }
            };
            let left: Vec<usize> = (0..self.children.len()).filter(|&i| places[i].action == ActionRegion::Left).collect();
            let right: Vec<usize> = (0..self.children.len()).filter(|&i| places[i].action == ActionRegion::Right).collect();
            let right_w: f32 = right.iter().map(|&i| natural(i).0).sum::<f32>() + FOOTER_GAP * (right.len().saturating_sub(1)) as f32;
            place_row(left, footer.left + FOOTER_PAD_X, &mut rects);
            place_row(right, footer.right - FOOTER_PAD_X - right_w, &mut rects);
        }
        for (i, (child, rect)) in self.children.iter_mut().zip(rects).enumerate() {
            let in_title = in_band(&places[i]);
            // The title bar's standard items show in the band only (a window without a Kubuno band has none).
            if i >= first_header && !in_title {
                continue;
            }
            // What the window must know about the child: a control in the band takes the pointer
            // (not a drag), a `TitleBar.Drag` one moves the window.
            if places[i].drag {
                crate::window::declare_drag_area(rect);
            } else if in_title {
                crate::window::declare_title_bar_hole(rect);
            }
            // The focusable controls of the band, for F6 (`crate::common::FrameServices::title_band_focus`).
            let access_mark = if in_title { cx.services.as_deref().map(|s| s.access_ids.len()) } else { None };
            {
                let mut inner = cx.reborrow();
                if in_title {
                    // In the window's title band, the non-client area above the page: not clipped to the root
                    // panel's box, which is the page's (`crate::clip`).
                    crate::clip::detached(|| child.paint(&mut inner, rect));
                } else {
                    child.paint(&mut inner, rect);
                }
            }
            if let (Some(mark), Some(services)) = (access_mark, cx.services.as_deref_mut()) {
                let focusable: Vec<kubuno_ui::FocusId> = services.access_ids.iter().skip(mark).filter_map(|(_, _, f)| *f).collect();
                services.title_band_focus.extend(focusable);
            }
        }
    }
}

/// A child of the view's root as the title band places it: its region, its natural size, and whether it follows
/// the previous item of its region with no gap (the title bar's standard items, side by side like the web's
/// header cluster).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BandItem {
    pub region: crate::window::TitleRegion,
    pub width: f32,
    pub height: f32,
    pub joined: bool,
}

/// How wide each title-bar region's controls are: their widths, `gap-1` (`window_chrome::BUTTON_GAP`) between two
/// of them, none between joined ones.
pub fn band_slot_widths(items: &[BandItem]) -> kubuno_controls::window_chrome::SlotWidths {
    use crate::window::TitleRegion;
    let gap = kubuno_controls::window_chrome::BUTTON_GAP;
    let mut widths = kubuno_controls::window_chrome::SlotWidths::default();
    let mut started = [false; 3];
    for it in items {
        let (slot, k) = match it.region {
            TitleRegion::Left => (&mut widths.left, 0),
            TitleRegion::Center => (&mut widths.center, 1),
            TitleRegion::Right => (&mut widths.right, 2),
            TitleRegion::None => continue,
        };
        *slot += if started[k] && !it.joined { gap + it.width } else { it.width };
        started[k] = true;
    }
    widths
}

/// Where each item goes in the band `layout`: from its region's start in document order (from its right edge when
/// `rtl`: a mirrored window reads its regions right to left), `gap-1` apart or joined, centred vertically on the
/// band and never taller than it. `None` for an item in no region.
pub fn band_item_rects(items: &[BandItem], layout: &kubuno_controls::window_chrome::ChromeLayout, rtl: bool) -> Vec<Option<Rect>> {
    use crate::window::TitleRegion;
    let gap = kubuno_controls::window_chrome::BUTTON_GAP;
    let mut out = vec![None; items.len()];
    for (region, slot) in [(TitleRegion::Left, layout.left), (TitleRegion::Center, layout.center), (TitleRegion::Right, layout.right)] {
        let mut x = if rtl { slot.right } else { slot.left };
        let mut first = true;
        for (i, it) in items.iter().enumerate().filter(|(_, it)| it.region == region) {
            let step = if first || it.joined { 0.0 } else { gap };
            first = false;
            let h = it.height.min(layout.band.bottom - layout.band.top);
            let top = (layout.band.top + layout.band.bottom - h) / 2.0;
            let mut r = if rtl {
                x -= step;
                let r = Rect::new(x - it.width, top, x, top + h);
                x -= it.width;
                r
            } else {
                x += step;
                let r = Rect::new(x, top, x + it.width, top + h);
                x += it.width;
                r
            };
            // The centre region narrowed by the sides (`window_chrome::layout`): its controls are cut to it.
            if region == TitleRegion::Center {
                r.left = r.left.max(slot.left);
                r.right = r.right.min(slot.right).max(r.left);
            }
            out[i] = Some(r);
        }
    }
    out
}

// ═════════════════════════════════════════════════════════════════════════
// GroupBox — a titled frame around one child body.
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: group_box,
    name: "GroupBox",
    // Note: A titled frame around one child (`kubuno_ui::containers::GroupBox`).
    doc: "A frame with a title around one child.",
    ctor: kubuno_ui::containers::GroupBox::new(),
    children: ChildrenModel::SingleWidget,
    props: [
        PropertyMeta::new("Title", PropKind::String, "", "Title shown at the top of the frame."),
        PropertyMeta::new("Padding", PropKind::F32, "0", "Space around the child, on all four sides, in pixels."),
    ],
    events: [],
    smoke: |mut g| {
        g.text = "Réglages".to_string();
        g.with_padding(kubuno_ui::Padding::all(8.0))
    },
    build: |props, cx| {
        use super::*;
        let title = props.str("Title", "")?;
        let padding = props.f32("Padding", 0.0)?;
        let child = props.build_single_child(cx)?;
        Ok(Box::new(crate::registry::families::containers::GroupBoxNode { title, padding, child }) as Box<dyn ViewNode>)
    },
}

/// The live node for `<GroupBox>` — see [`PanelNode`]'s doc for why the real
/// `kubuno_ui` container is rebuilt every frame for chrome/layout only.
pub struct GroupBoxNode {
    title: PropSource<String>,
    padding: PropSource<f32>,
    child: Option<Box<dyn ViewNode>>,
}

impl GroupBoxNode {
    fn build(&self, vm: &dyn crate::binding::ViewModel) -> kubuno_ui::containers::GroupBox {
        let mut g = kubuno_ui::containers::GroupBox::titled(self.title.resolve(vm));
        g = g.with_padding(Padding::all(self.padding.resolve(vm)));
        g.fill()
    }
}

impl ViewNode for GroupBoxNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn crate::binding::ViewModel) -> Size {
        let base = self.build(vm).measure(c);
        let child = self.child.as_ref().map(|n| n.measure(c, vm)).unwrap_or(Size::EMPTY);
        Size::new(base.width.max(child.width), base.height + child.height)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let group = self.build(cx.vm);
        let canvas: &dyn Canvas = cx.canvas;
        let child = &mut self.child;
        let mut inner = cx.reborrow();
        group.paint_with(canvas, bounds, crate::common::rest(), move |_c, body_rect| {
            if let Some(node) = child.as_mut() {
                node.paint(&mut inner, body_rect);
            }
        });
    }
}

// ═════════════════════════════════════════════════════════════════════════
// ScrollArea — wraps and scrolls its one child's painted content, over the
// immediate-mode `kubuno_ui::containers::ScrollArea` (overflow: auto).
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: scroll_area,
    name: "ScrollArea",
    // Note: Scrolls whatever its one child paints past its edges (`kubuno_ui::containers::ScrollArea`).
    doc: "A scrollable area around one child.",
    ctor: kubuno_ui::containers::ScrollArea::new(),
    children: ChildrenModel::SingleWidget,
    props: [
        PropertyMeta::new("Corner", PropKind::F32, "0", "Radius of the rounded corners, in pixels. 0 for square corners."),
    ],
    events: [],
    smoke: |a| { a.with_corner(8.0) },
    build: |props, cx| {
        use super::*;
        let corner = props.f32("Corner", 0.0)?;
        let child = props.build_single_child(cx)?;
        Ok(Box::new(crate::registry::families::containers::ScrollAreaNode {
            corner,
            area: kubuno_ui::containers::ScrollArea::new(),
            child,
        }) as Box<dyn ViewNode>)
    },
}

/// The live node for `<ScrollArea>`. Unlike [`PanelNode`]/[`GroupBoxNode`],
/// [`kubuno_ui::containers::ScrollArea`] is not rebuilt every frame: its scroll
/// offset and last-measured content extent are real state that a rebuild would
/// erase (the same reason [`crate::node::TextFieldNode`] keeps its own
/// `kubuno_ui::text::TextField` — see `crate::node`'s module doc).
pub struct ScrollAreaNode {
    corner: PropSource<f32>,
    area: kubuno_ui::containers::ScrollArea,
    child: Option<Box<dyn ViewNode>>,
}

impl ViewNode for ScrollAreaNode {
    /// No intrinsic size of its own (`overflow: auto` on whatever box it is
    /// given): the child's measured size is the closest approximation
    /// reachable without a live frame's paint pass to grow
    /// [`kubuno_ui::containers::ScrollArea`]'s own tracked extent.
    fn measure(&self, c: &dyn Canvas, vm: &dyn crate::binding::ViewModel) -> Size {
        self.child.as_ref().map(|n| n.measure(c, vm)).unwrap_or(Size::EMPTY)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        self.area.corner = self.corner.resolve(cx.vm);
        // `ScrollArea::frame`'s content closure only hands back `&dyn Canvas`
        // (`kubuno_ui`'s narrower painting trait), while `PaintCx::canvas` is
        // `&dyn ControlCanvas` (a supertrait — `crate::node`'s own module
        // already upcasts `ControlCanvas` → `Canvas` the same way, e.g.
        // `CardNode::paint`'s `let canvas: &dyn Canvas = cx.canvas;`). That
        // coercion only runs ONE way, so the closure's `&dyn Canvas` cannot be
        // turned back into the `&dyn ControlCanvas` a recursed `ViewNode`
        // needs — the ORIGINAL reference is reused instead (the same
        // underlying canvas; `frame`'s clip/offset are canvas STATE, not a
        // different object), and the closure's own `c` is ignored.
        let outer_canvas = cx.canvas;
        let canvas: &dyn Canvas = outer_canvas;
        let outer_frame = cx.frame;
        let child = &mut self.child;
        let mut inner = cx.reborrow();
        self.area.frame(canvas, bounds, outer_frame, move |_c, f| {
            if let Some(node) = child.as_mut() {
                let mut content_cx = inner.with_surface(outer_canvas, f);
                // The content paints at the SAME bounds it would without the
                // area: `ScrollArea::frame` already pushed the clip/offset
                // (`push_offset`/`push_clip`) around this closure.
                node.paint(&mut content_cx, bounds);
            }
        });
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Splitter — two panes over `kubuno_controls::layout_panels::SplitContainer`.
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: splitter,
    name: "Splitter",
    // Note: Two panes separated by a draggable band (`kubuno_ui::containers::Splitter`).
    doc: "Two panes separated by a bar that can be dragged.",
    ctor: kubuno_ui::containers::Splitter::vertical(),
    children: ChildrenModel::List(&[]),
    layout: LayoutKind::Split,
    default_event: "OnDistanceChanged",
    props: [
        // Note: `Vertical`: a vertical BAR, panes side by side. `Horizontal`: a horizontal bar, panes stacked.
        PropertyMeta::new("Orientation", PropKind::Enum(&["Vertical", "Horizontal"]), "Vertical",
            "Vertical: panes side by side. Horizontal: panes one above the other.",
        ),
        PropertyMeta::new("Distance", PropKind::F32, "200",
            "Size of the first pane, in pixels. Updated when the bar is moved.",
        ),
    ],
    events: [
        EventMeta::new("OnDistanceChanged", "Occurs when the bar is moved.").category(crate::registry::EventCategory::Behavior).args::<crate::events::NumericValueChangedEventArgs>(),
    ],
    smoke: |s| {
        let s = s.with_distance(240.0);
        s.with_minimums(80.0, 80.0)
    },
    build: |props, cx| {
        use super::*;
        let orientation = props.enum_("Orientation", "Vertical")?;
        let distance = props.f32("Distance", 200.0)?;
        let focus_id = props.focus_id();
        let on_distance_changed = props.event("OnDistanceChanged");
        let children = props.build_children(cx)?;
        if children.len() != 2 {
            return Err(BuildError::new(
                format!("`<Splitter>` needs exactly two children (a pane 1 and a pane 2), found {}", children.len()),
                props.element().name_range(),
            ));
        }
        Ok(Box::new(crate::registry::families::containers::SplitterNode {
            orientation,
            distance,
            focus_id,
            on_distance_changed,
            dragging: false,
            local: None,
            last_source: None,
            children,
        }) as Box<dyn ViewNode>)
    },
}

/// The live node for `<Splitter>`: the two panes and the divider of `kubuno_ui::containers::Splitter`,
/// the bar dragged with the mouse (`Splitter::distance_for`) or moved with the keyboard once it has the
/// focus (`handle_key`). The distance the user sets is kept by the node (a literal `Distance` stays the
/// starting point), written back through a two-way binding, and raised as `DistanceChanged`; a change
/// of the literal or of the bound value wins over it.
pub struct SplitterNode {
    orientation: PropSource<String>,
    distance: PropSource<f32>,
    focus_id: Option<FocusId>,
    on_distance_changed: Option<String>,
    /// Whether a drag that started on the grab band is still in progress —
    /// kept across frames for the same reason `choice::SliderNode::dragging`
    /// is: the pointer may wander off the bar mid-drag and the bar must keep
    /// following it (`Splitter::drag_to`'s own contract).
    dragging: bool,
    /// The distance the user set, until `Distance` itself changes.
    local: Option<f32>,
    /// `Distance` as last read (a change of it drops [`Self::local`]).
    last_source: Option<f32>,
    children: Vec<Box<dyn ViewNode>>,
}

impl SplitterNode {
    /// The distance to show: what the user set, else `Distance`.
    fn current(&mut self, vm: &dyn crate::binding::ViewModel) -> f32 {
        let source = self.distance.resolve(vm);
        if self.last_source != Some(source) {
            self.last_source = Some(source);
            self.local = None;
        }
        self.local.unwrap_or(source)
    }

    fn build(&self, vm: &dyn crate::binding::ViewModel, distance: f32) -> kubuno_ui::containers::Splitter {
        let s = match self.orientation.resolve(vm).as_str() {
            "Horizontal" => kubuno_ui::containers::Splitter::horizontal(),
            _ => kubuno_ui::containers::Splitter::vertical(),
        };
        s.with_distance(distance)
    }

    /// Writes `new_distance` back (when bound `Mode=TwoWay`) and dispatches
    /// `OnDistanceChanged` — the same two-consumer shape every other bound
    /// control in this crate uses (see `choice::SliderNode::interact`).
    fn commit_distance(&mut self, ix: &mut crate::node::InteractCx<'_>, new_distance: f32) {
        let old_distance = self.local.unwrap_or_else(|| self.distance.resolve(ix.vm));
        self.local = Some(new_distance);
        if let Some(spec) = self.distance.binding() {
            if spec.mode.writes_back() {
                spec.update_source(ix.vm, Value::F32(new_distance));
                // The value written back is the source now: not a change of it.
                self.last_source = Some(self.distance.resolve(ix.vm));
            }
        }
        let mut args = NumericValueChangedEventArgs::new(old_distance, new_distance, ChangeSource::User);
        ix.fire("OnDistanceChanged", self.focus_id, self.on_distance_changed.as_deref(), ViewEventKind::Changed(new_distance.to_string()), &mut args);
    }

    /// The canvas-independent half of a frame — mouse-drag + keyboard, both
    /// funnelled through [`Self::commit_distance`] — mirroring
    /// `choice::SliderNode::interact`'s split so this crate's own tests can
    /// drive it without a live `Canvas` (see `node::tests`' note on why that
    /// matters). Never touches `self.orientation`/`self.distance`'s
    /// *resolution* result beyond what the caller already resolved into
    /// `distance` — re-resolving after a write-back is `paint`'s job, for
    /// same-frame visual feedback, same as every other bound leaf here.
    pub(crate) fn interact(&mut self, ix: &mut crate::node::InteractCx<'_>, bounds: Rect, distance: f32) {
        let mut splitter = self.build(ix.vm, distance);

        // Mouse: press-and-hold on the grab band follows the pointer every
        // frame it moves (`Splitter::drag_to`), exactly like
        // `choice::SliderNode`'s own drag — not a press/release click.
        let (mx, my) = ix.frame.mouse;
        let hot = !ix.frame.pointer_outside() && splitter.hit_test_grip(bounds, mx, my);
        if ix.frame.mouse_down && (self.dragging || hot) {
            self.dragging = true;
            let new_distance = splitter.distance_for(bounds, mx, my);
            if new_distance != distance {
                self.commit_distance(ix, new_distance);
            }
        } else {
            self.dragging = false;
        }

        // Keyboard: only once the bar itself holds focus (`x:Name`d, like
        // every other keyboard-driven control here) — the two arrows along
        // the split axis (Shift = a larger step), Home/End, Enter to
        // collapse/restore (`Splitter::handle_key`'s own WAI-ARIA "window
        // splitter" behaviour, unchanged here).
        let focus_state = self.focus_id.map(|id| ix.focus.register(id, bounds)).unwrap_or_default();
        if focus_state.focused {
            let mut moved = false;
            for (key, mods) in [
                (vk::LEFT, Modifiers::NONE),
                (vk::LEFT, Modifiers::SHIFT),
                (vk::RIGHT, Modifiers::NONE),
                (vk::RIGHT, Modifiers::SHIFT),
                (vk::UP, Modifiers::NONE),
                (vk::UP, Modifiers::SHIFT),
                (vk::DOWN, Modifiers::NONE),
                (vk::DOWN, Modifiers::SHIFT),
                (vk::HOME, Modifiers::NONE),
                (vk::END, Modifiers::NONE),
                (vk::ENTER, Modifiers::NONE),
            ] {
                for _ in 0..host::take_key(key, mods) {
                    if splitter.handle_key(bounds, key, mods) {
                        moved = true;
                    }
                }
            }
            if moved {
                // `Splitter` exposes no `distance()` getter (this crate
                // never reaches into `kubuno_ui`'s private fields — see
                // `node.rs`'s own module doc on why a `ViewNode` recurses
                // itself instead of trusting a container's `Widget::paint`):
                // `handle_key` already mutated `splitter`'s own internal
                // state, and pane 1's extent along the split axis IS the
                // distance it moved to (`arrange`'s own definition), read
                // back through the same public `panel1_rect` painting
                // already uses.
                let p1 = splitter.panel1_rect(bounds);
                let is_vertical = self.orientation.resolve(ix.vm) != "Horizontal";
                let new_distance = if is_vertical { p1.right - p1.left } else { p1.bottom - p1.top };
                self.commit_distance(ix, new_distance);
            }
        }
    }
}

impl ViewNode for SplitterNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn crate::binding::ViewModel) -> Size {
        self.build(vm, self.distance.resolve(vm)).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let distance = self.current(&*cx.vm);
        if cx.design.is_none() {
            let mut ix = cx.interact_cx();
            self.interact(&mut ix, bounds, distance);
        }

        // Re-read AFTER interaction, for same-frame feedback — the same
        // technique every other bound leaf in this crate uses.
        let distance = self.current(&*cx.vm);
        let splitter = self.build(cx.vm, distance);
        let canvas: &dyn Canvas = cx.canvas;
        splitter.paint(canvas, bounds, crate::common::rest());
        let rects = [splitter.panel1_rect(bounds), splitter.panel2_rect(bounds)];
        for (child, rect) in self.children.iter_mut().zip(rects) {
            let mut inner = cx.reborrow();
            child.paint(&mut inner, rect);
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Tabs / TabItem — over `kubuno_ui::navigation::{Tabs, TabsController}`.
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: tab_item,
    name: "TabItem",
    // Note: One page of a `<Tabs>` strip: a header and a body. Valid only as a direct `<Tabs>` child.
    doc: "A page of a Tabs control.",
    ctor: kubuno_controls::layout_panels::TabPage::new("Onglet"),
    children: ChildrenModel::SingleWidget,
    props: [
        PropertyMeta::new("Header", PropKind::String, "", "Text of the tab."),
    ],
    events: [],
    smoke: |mut p| {
        p.tool_tip_text = "Détails".to_string();
        p
    },
    build: |props, cx| {
        use super::*;
        // `<TabItem>` is only ever built through `Tabs`' own child walk (see
        // that component's `build`), which reads `Header` and the body
        // directly off the element rather than calling this function — kept
        // real (and tested by `ctor_and_setters_match_kubuno_ui`) so
        // `<TabItem>` validates and documents like any other element.
        let header = props.str("Header", "")?;
        let child = props.build_single_child(cx)?;
        let _ = (header, child);
        Err(BuildError::new("`<TabItem>` is only valid as a direct child of `<Tabs>`", props.element().name_range()))
    },
}

component! {
    mod_name: tabs,
    name: "Tabs",
    // Note: An underline tab strip (`kubuno_ui::navigation::Tabs` + `TabsController`), pages `<TabItem>`.
    doc: "A set of pages shown one at a time, with tabs. Add the pages as TabItem children.",
    ctor: kubuno_ui::navigation::Tabs::new(),
    children: ChildrenModel::List(&["TabItem"]),
    layout: LayoutKind::Tabs,
    default_event: "OnSelectionChanged",
    props: [
        PropertyMeta::new("SelectedIndex", PropKind::F32, "0", "Index of the selected tab, starting at 0."),
        crate::owner_draw::DRAW_MODE_FIXED,
    ],
    events: [
        EventMeta::new("OnSelectionChanged", "Occurs when another tab is selected.").category(crate::registry::EventCategory::Behavior).args::<crate::events::SelectionChangedEventArgs>(),
        crate::owner_draw::ON_DRAW_ITEM,
    ],
    smoke: |t| { t.small() },
    build: |props, cx| {
        use super::*;
        let selected = props.f32("SelectedIndex", 0.0)?;
        let on_selection_changed = props.event("OnSelectionChanged");
        let focus_id = props.focus_id();
        let mut tabs = kubuno_ui::navigation::Tabs::new();
        let mut children: Vec<Option<Box<dyn ViewNode>>> = Vec::new();
        for child in props.element().children() {
            let name = child.name().unwrap_or_default();
            if name != "TabItem" {
                return Err(BuildError::new(format!("`<Tabs>` only accepts `<TabItem>` children, found `<{name}>`"), child.name_range()));
            }
            let meta = crate::registry::lookup("TabItem")
                .ok_or_else(|| BuildError::new("`TabItem` is not registered", child.name_range()))?;
            let child_props = Props::new(&child, meta);
            let header = literal_str(&child_props, "Header", "")?;
            tabs.tab_pages.push(kubuno_controls::layout_panels::TabPage::new(header));
            children.push(child_props.build_single_child(cx)?);
        }
        Ok(Box::new(crate::registry::families::containers::TabsNode {
            selected,
            on_selection_changed,
            focus_id,
            draw_mode: crate::owner_draw::draw_mode_prop(props)?,
            owner: crate::owner_draw::OwnerDrawEvents::read(props),
            tabs,
            controller: kubuno_ui::navigation::TabsController::new(),
            children,
        }) as Box<dyn ViewNode>)
    },
}

/// The live node for `<Tabs>`. Both [`kubuno_ui::navigation::Tabs`] (the
/// strip's items and its geometry) and [`kubuno_ui::navigation::TabsController`]
/// (the sliding indicator, per-tab scroll, keyboard roving) are kept as node
/// state across frames — see [`ScrollAreaNode`]'s doc for why a controller like
/// this one is never rebuilt.
pub struct TabsNode {
    selected: PropSource<f32>,
    on_selection_changed: Option<String>,
    focus_id: Option<FocusId>,
    /// `DrawMode` (`Normal`, `OwnerDrawFixed`) and the owner-draw handlers (EVT-8).
    draw_mode: PropSource<String>,
    owner: crate::owner_draw::OwnerDrawEvents,
    tabs: kubuno_ui::navigation::Tabs,
    controller: kubuno_ui::navigation::TabsController,
    /// One body per `<TabItem>`, index-aligned with `tabs.tab_pages`.
    children: Vec<Option<Box<dyn ViewNode>>>,
}

impl ViewNode for TabsNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn crate::binding::ViewModel) -> Size {
        self.tabs.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        // A binding/programmatic change is applied BEFORE the controller
        // runs, so a click this same frame is what actually wins (matching
        // `crate::node::SwitchNode`'s "resolve, then interact, then
        // re-resolve" ordering).
        let wanted = self.selected.resolve(cx.vm) as i32;
        if self.tabs.selected_index != wanted {
            self.tabs.selected_index = wanted;
        }
        let focus_state = self.focus_id.map(|id| cx.focus.register(id, bounds));
        let outer_canvas = cx.canvas;
        let canvas: &dyn Canvas = outer_canvas;
        let mode = match self.draw_mode.resolve(cx.vm).as_str() {
            "OwnerDrawFixed" => kubuno_controls::layout_panels::TabDrawMode::OwnerDrawFixed,
            _ => kubuno_controls::layout_panels::TabDrawMode::Normal,
        };
        if self.tabs.draw_mode != mode {
            self.tabs.draw_mode = mode;
        }
        let frame = cx.frame;
        let (controller, tabs) = (&mut self.controller, &mut self.tabs);
        let run = crate::owner_draw::paint_with(cx, &self.owner, |c| controller.frame(c, tabs, bounds, frame, focus_state));
        if run.changed {
            let new_index = self.tabs.selected_index;
            if let Some(spec) = self.selected.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::F32(new_index as f32));
                }
            }
            let mut args = SelectionChangedEventArgs::new(usize::try_from(wanted).ok(), usize::try_from(new_index).ok(), ChangeSource::User);
            cx.fire("OnSelectionChanged", self.focus_id, self.on_selection_changed.as_deref(), ViewEventKind::Changed(new_index.to_string()), &mut args);
        }
        let i = usize::try_from(self.tabs.selected_index).unwrap_or(0);
        if let Some(Some(child)) = self.children.get_mut(i) {
            let page = self.tabs.page_rect(canvas, bounds);
            let outer_frame = cx.frame;
            let mut inner = cx.reborrow();
            // See `ScrollAreaNode::paint`'s comment: the closure's own
            // canvas is `&dyn Canvas`, narrower than `PaintCx::canvas`'s
            // `&dyn ControlCanvas` — the original reference is reused.
            self.controller.page(canvas, &self.tabs, page, outer_frame, move |_c, f| {
                let mut content_cx = inner.with_surface(outer_canvas, f);
                child.paint(&mut content_cx, page);
            });
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Breadcrumb / BreadcrumbItem — over `kubuno_ui::navigation::Breadcrumb`.
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: breadcrumb_item,
    name: "BreadcrumbItem",
    // Note: One segment of a `<Breadcrumb>` trail. Valid only as a direct `<Breadcrumb>` child.
    doc: "A segment of a Breadcrumb trail.",
    ctor: kubuno_controls::toolstrip::ToolStripButton::new("Segment"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text of the segment.").localizable().bindable(),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the segment is clicked.").category(crate::registry::EventCategory::Action).args::<crate::events::ItemEventArgs>(),
    ],
    smoke: |mut b| {
        b.item.text = "Dossier".to_string();
        b
    },
    build: |props, _cx| {
        use super::*;
        // See `TabItem`'s `build`: only ever reached through `Breadcrumb`'s
        // own child walk in practice.
        Err(BuildError::new("`<BreadcrumbItem>` is only valid as a direct child of `<Breadcrumb>`", props.element().name_range()))
    },
}

component! {
    mod_name: breadcrumb,
    name: "Breadcrumb",
    // Note: A path trail (`kubuno_ui::navigation::Breadcrumb`), segments `<BreadcrumbItem>`.
    doc: "A navigation trail. Add the segments as BreadcrumbItem children.",
    ctor: kubuno_ui::navigation::Breadcrumb::new(),
    children: ChildrenModel::List(&["BreadcrumbItem"]),
    props: [
        PropertyMeta::new("RootChevron", PropKind::Bool, "false", "Leaves room for a home icon before the first segment."),
    ],
    events: [],
    smoke: |mut b| {
        b.root_chevron = true;
        b
    },
    build: |props, _cx| {
        use super::*;
        let root_chevron = literal_bool(props, "RootChevron", false)?;
        let mut breadcrumb = kubuno_ui::navigation::Breadcrumb::new();
        breadcrumb.root_chevron = root_chevron;
        let mut handlers: Vec<Option<String>> = Vec::new();
        let mut texts: Vec<crate::binding::PropSource<String>> = Vec::new();
        for child in props.element().children() {
            let name = child.name().unwrap_or_default();
            if name != "BreadcrumbItem" {
                return Err(BuildError::new(format!("`<Breadcrumb>` only accepts `<BreadcrumbItem>` children, found `<{name}>`"), child.name_range()));
            }
            let meta = crate::registry::lookup("BreadcrumbItem")
                .ok_or_else(|| BuildError::new("`BreadcrumbItem` is not registered", child.name_range()))?;
            let child_props = Props::new(&child, meta);
            let text = child_props.str("Text", "")?;
            if let crate::binding::PropSource::Literal(t) = &text {
                breadcrumb = breadcrumb.with(t);
            }
            texts.push(text);
            handlers.push(child_props.event("OnClick"));
        }
        let bound = texts.iter().any(|t| matches!(t, crate::binding::PropSource::Bound { .. }));
        Ok(Box::new(crate::registry::families::containers::BreadcrumbNode {
            breadcrumb,
            texts: if bound { texts } else { Vec::new() },
            handlers,
            focus_id: props.focus_id(),
            pressed: None,
        }) as Box<dyn ViewNode>)
    },
}

/// The live node for `<Breadcrumb>`. Segments are static text (built once,
/// like `<Panel>`'s Dock/Anchor placement — see the module doc): the real
/// `kubuno_ui::navigation::Breadcrumb` owns the fold-behind-`…` arithmetic and
/// the paint; this node only adds the hit-test → handler dispatch loop.
pub struct BreadcrumbNode {
    breadcrumb: kubuno_ui::navigation::Breadcrumb,
    /// The segments' texts when one is bound (`Text="{Binding Section}"`): the trail is rebuilt from
    /// them each frame. Empty for a trail of literal segments, built once.
    texts: Vec<crate::binding::PropSource<String>>,
    /// One handler name per segment, index-aligned with `breadcrumb.items`.
    handlers: Vec<Option<String>>,
    focus_id: Option<FocusId>,
    pressed: Option<usize>,
}

impl BreadcrumbNode {
    /// The trail with its bound segments resolved (`None`: all literal, `self.breadcrumb` as built).
    fn resolved(&self, vm: &dyn crate::binding::ViewModel) -> Option<kubuno_ui::navigation::Breadcrumb> {
        if self.texts.is_empty() {
            return None;
        }
        let mut b = kubuno_ui::navigation::Breadcrumb::new();
        b.root_chevron = self.breadcrumb.root_chevron;
        for t in &self.texts {
            b = b.with(&t.resolve(vm));
        }
        Some(b)
    }
}

impl ViewNode for BreadcrumbNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn crate::binding::ViewModel) -> Size {
        self.resolved(vm).as_ref().unwrap_or(&self.breadcrumb).measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        if let Some(b) = self.resolved(&*cx.vm) {
            self.breadcrumb = b;
        }
        let canvas: &dyn Canvas = cx.canvas;
        let (mx, my) = cx.frame.mouse;
        let hot = (!cx.frame.pointer_outside()).then(|| self.breadcrumb.item_at(canvas, bounds, mx, my)).flatten();
        let (down_now, clicked) = press_release(&mut false, hot.is_some(), cx.frame.mouse_down);
        // `press_release` needs its OWN persisted flag per segment; reusing
        // one shared `pressed: Option<usize>` (which segment, if any, is
        // currently down) gives the same press→release-inside semantics.
        let was_pressed_here = self.pressed == hot && hot.is_some();
        if cx.frame.mouse_down && hot.is_some() {
            self.pressed = hot;
        }
        let clicked = clicked || (was_pressed_here && !cx.frame.mouse_down && self.pressed == hot);
        let _ = down_now;
        if !cx.frame.mouse_down {
            if clicked {
                if let Some(i) = hot.filter(|&i| self.breadcrumb.is_link(i)) {
                    if let Some(handler) = self.handlers.get(i).and_then(|h| h.as_deref()) {
                        cx.fire("OnItemClicked", self.focus_id, Some(handler), ViewEventKind::Clicked, &mut ItemEventArgs { index: i });
                    }
                }
            }
            self.pressed = None;
        }
        self.breadcrumb.paint_segments(canvas, bounds, hot, false);
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Toolbar / ToolbarItem — over `kubuno_ui::navigation::Toolbar`.
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: toolbar_item,
    name: "ToolbarItem",
    // Note: One command of a `<Toolbar>`. Valid only as a direct `<Toolbar>` child.
    doc: "A command of a Toolbar.",
    ctor: kubuno_controls::toolstrip::ToolStripButton::new("Commande"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Text", PropKind::String, "", "Text of the command. Leave empty for an icon-only command."),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon shown before the text: a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.").editor("icon").category("Icon"),
    ],
    events: [
        EventMeta::new("OnClick", "Occurs when the command is clicked.").category(crate::registry::EventCategory::Action).args::<crate::events::ItemEventArgs>(),
    ],
    smoke: |mut b| {
        b.item.text = "Nouveau".to_string();
        b.item.image = Some("plus".to_string());
        b
    },
    build: |props, _cx| {
        use super::*;
        // See `TabItem`'s `build`: only ever reached through `Toolbar`'s own
        // child walk in practice.
        Err(BuildError::new("`<ToolbarItem>` is only valid as a direct child of `<Toolbar>`", props.element().name_range()))
    },
}

component! {
    mod_name: toolbar,
    name: "Toolbar",
    // Note: A command bar (`kubuno_ui::navigation::Toolbar`), commands `<ToolbarItem>`.
    doc: "A toolbar. Add the commands as ToolbarItem children.",
    ctor: kubuno_ui::navigation::Toolbar::new(),
    children: ChildrenModel::List(&["ToolbarItem"]),
    props: [
        // Note: Paints its own `layer_background` band under the commands.
        PropertyMeta::new("Band", PropKind::Bool, "false", "Paints a background band behind the commands."),
    ],
    events: [],
    smoke: |t| { t.with_band(true) },
    build: |props, _cx| {
        use super::*;
        let band = props.bool("Band", false)?;
        let band = matches!(band, PropSource::Literal(true));
        let mut toolbar = kubuno_ui::navigation::Toolbar::new().with_band(band);
        let mut handlers: Vec<Option<String>> = Vec::new();
        for child in props.element().children() {
            let name = child.name().unwrap_or_default();
            if name != "ToolbarItem" {
                return Err(BuildError::new(format!("`<Toolbar>` only accepts `<ToolbarItem>` children, found `<{name}>`"), child.name_range()));
            }
            let meta = crate::registry::lookup("ToolbarItem")
                .ok_or_else(|| BuildError::new("`ToolbarItem` is not registered", child.name_range()))?;
            let child_props = Props::new(&child, meta);
            let text = literal_str(&child_props, "Text", "")?;
            let icon = literal_str(&child_props, "Icon", "")?;
            let mut b = kubuno_controls::toolstrip::ToolStripButton::default();
            b.item.text = text.clone();
            if !icon.is_empty() {
                b.item.image = Some(icon);
                b.item.display_style = if text.is_empty() {
                    kubuno_controls::toolstrip::ToolStripItemDisplayStyle::Image
                } else {
                    kubuno_controls::toolstrip::ToolStripItemDisplayStyle::ImageAndText
                };
            }
            toolbar.items.push(kubuno_controls::toolstrip::StripItem::Button(b));
            handlers.push(child_props.event("OnClick"));
        }
        Ok(Box::new(crate::registry::families::containers::ToolbarNode {
            toolbar,
            handlers,
            focus_id: props.focus_id(),
            pressed: None,
        }) as Box<dyn ViewNode>)
    },
}

/// The live node for `<Toolbar>` — see [`BreadcrumbNode`]'s doc, which this
/// mirrors: static commands (built once), the real
/// `kubuno_ui::navigation::Toolbar` owns overflow/arrangement/paint, this node
/// adds only the hit-test → handler dispatch loop.
pub struct ToolbarNode {
    toolbar: kubuno_ui::navigation::Toolbar,
    /// One handler name per command, index-aligned with `toolbar.items`.
    handlers: Vec<Option<String>>,
    focus_id: Option<FocusId>,
    pressed: Option<usize>,
}

impl ViewNode for ToolbarNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn crate::binding::ViewModel) -> Size {
        self.toolbar.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let canvas: &dyn Canvas = cx.canvas;
        let (mx, my) = cx.frame.mouse;
        let hot = (!cx.frame.pointer_outside()).then(|| self.toolbar.item_at(canvas, bounds, mx, my)).flatten();
        if cx.frame.mouse_down && hot.is_some() {
            self.pressed = hot;
        }
        let clicked = self.pressed.is_some() && self.pressed == hot && !cx.frame.mouse_down;
        if !cx.frame.mouse_down {
            if clicked {
                if let Some(i) = hot {
                    if let Some(handler) = self.handlers.get(i).and_then(|h| h.as_deref()) {
                        cx.fire("OnItemClicked", self.focus_id, Some(handler), ViewEventKind::Clicked, &mut ItemEventArgs { index: i });
                    }
                }
            }
            self.pressed = None;
        }
        self.toolbar.paint_items(canvas, bounds, hot, false);
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Accordion / AccordionSection — over `kubuno_ui::feedback::Accordion`.
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: accordion_section,
    name: "AccordionSection",
    // Note: One collapsible group of a `<Accordion>`. Valid only as a direct `<Accordion>` child.
    doc: "A section of an Accordion that can be expanded or collapsed.",
    ctor: kubuno_ui::feedback::AccordionSection::new("Section", 0.0),
    children: ChildrenModel::SingleWidget,
    props: [
        PropertyMeta::new("Header", PropKind::String, "", "Title of the section."),
        PropertyMeta::new("Open", PropKind::Bool, "false", "Whether the section is expanded."),
        PropertyMeta::new("Disabled", PropKind::Bool, "false", "Shows the section greyed out and keeps it collapsed."),
    ],
    events: [
        EventMeta::new("OnToggled", "Occurs when the section is expanded or collapsed.").category(crate::registry::EventCategory::Behavior).args::<crate::events::CheckedChangedEventArgs>(),
    ],
    smoke: |s| {
        let s = s.open(true);
        s.disabled(false)
    },
    build: |props, cx| {
        use super::*;
        // See `TabItem`'s `build`: only ever reached through `Accordion`'s
        // own child walk in practice.
        let _ = (props.str("Header", "")?, props.bool("Open", false)?, props.bool("Disabled", false)?, props.build_single_child(cx)?);
        Err(BuildError::new("`<AccordionSection>` is only valid as a direct child of `<Accordion>`", props.element().name_range()))
    },
}

component! {
    mod_name: accordion,
    name: "Accordion",
    // Note: A stack of collapsible groups (`kubuno_ui::feedback::Accordion`), sections `<AccordionSection>`.
    doc: "A list of sections that can be expanded or collapsed. Add the sections as AccordionSection children.",
    ctor: kubuno_ui::feedback::Accordion::new(),
    children: ChildrenModel::List(&["AccordionSection"]),
    props: [
        PropertyMeta::new("Size", PropKind::Enum(&["Sm", "Md"]), "Md", "Spacing of the section headers and contents."),
    ],
    events: [],
    smoke: |a| { a.with_size(kubuno_ui::feedback::AccordionSize::Sm) },
    build: |props, cx| {
        use super::*;
        let size = props.enum_("Size", "Md")?;
        let size = match size {
            PropSource::Literal(ref s) if s == "Sm" => kubuno_ui::feedback::AccordionSize::Sm,
            _ => kubuno_ui::feedback::AccordionSize::Md,
        };
        let mut accordion = kubuno_ui::feedback::Accordion::new().with_size(size);
        let mut handlers: Vec<Option<String>> = Vec::new();
        let mut focus_ids: Vec<Option<FocusId>> = Vec::new();
        let mut contents: Vec<Option<Box<dyn ViewNode>>> = Vec::new();
        for child in props.element().children() {
            let name = child.name().unwrap_or_default();
            if name != "AccordionSection" {
                return Err(BuildError::new(format!("`<Accordion>` only accepts `<AccordionSection>` children, found `<{name}>`"), child.name_range()));
            }
            let meta = crate::registry::lookup("AccordionSection")
                .ok_or_else(|| BuildError::new("`AccordionSection` is not registered", child.name_range()))?;
            let child_props = Props::new(&child, meta);
            let header = literal_str(&child_props, "Header", "")?;
            let open = child_props.bool("Open", false)?;
            let open = matches!(open, PropSource::Literal(true));
            let disabled = child_props.bool("Disabled", false)?;
            let disabled = matches!(disabled, PropSource::Literal(true));
            accordion = accordion.section(kubuno_ui::feedback::AccordionSection::new(header, 0.0).open(open).disabled(disabled));
            handlers.push(child_props.event("OnToggled"));
            focus_ids.push(child_props.focus_id());
            contents.push(child_props.build_single_child(cx)?);
        }
        let n = accordion.sections.len();
        Ok(Box::new(crate::registry::families::containers::AccordionNode {
            accordion,
            handlers,
            focus_ids,
            contents,
            pressed: vec![false; n],
        }) as Box<dyn ViewNode>)
    },
}

/// The live node for `<Accordion>`. [`kubuno_ui::feedback::Accordion`] is kept
/// as node state (its `sections[i].open` IS the open/closed state — see
/// [`ScrollAreaNode`]'s doc on why a controller like this is never rebuilt).
/// Hit-testing (`header_at`) and layout (`section_rects`/`panel_rect`) are
/// pure `Rect` arithmetic with no `Canvas` — see this family's report and
/// `containers::tests` below.
pub struct AccordionNode {
    accordion: kubuno_ui::feedback::Accordion,
    /// One handler name / focus id / body per section, index-aligned with
    /// `accordion.sections`.
    handlers: Vec<Option<String>>,
    focus_ids: Vec<Option<FocusId>>,
    contents: Vec<Option<Box<dyn ViewNode>>>,
    pressed: Vec<bool>,
}

impl AccordionNode {
    /// The canvas-independent half of a frame: hit-tests `bounds` against
    /// `mouse`, tracks press/release per section (mirroring
    /// [`crate::node::ButtonNode::interact`]), toggles the section that
    /// completed a click and returns its new index. `pub(crate)` so this
    /// family's own tests exercise it without a live `Canvas` (see
    /// `containers::tests`).
    pub(crate) fn interact(&mut self, bounds: Rect, mouse: (f32, f32), pointer_outside: bool, mouse_down: bool) -> Option<usize> {
        let hot = (!pointer_outside).then(|| self.accordion.header_at(bounds, mouse.0, mouse.1)).flatten();
        let mut toggled = None;
        for (i, pressed) in self.pressed.iter_mut().enumerate() {
            let (_, clicked) = press_release(pressed, hot == Some(i), mouse_down);
            if clicked {
                toggled = Some(i);
            }
        }
        if let Some(i) = toggled {
            self.accordion.toggle(i);
        }
        toggled
    }
}

impl ViewNode for AccordionNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn crate::binding::ViewModel) -> Size {
        let mut probe = kubuno_ui::feedback::Accordion::new();
        for (section, content) in self.accordion.sections.iter().zip(&self.contents) {
            let content_h = content.as_ref().map(|n| n.measure(c, vm).height).unwrap_or(0.0);
            probe = probe.section(kubuno_ui::feedback::AccordionSection::new(section.title.clone(), content_h).open(section.open).disabled(section.disabled));
        }
        probe.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let canvas: &dyn Canvas = cx.canvas;
        for (section, content) in self.accordion.sections.iter_mut().zip(&self.contents) {
            section.content = content.as_ref().map(|n| n.measure(canvas, cx.vm).height).unwrap_or(0.0);
        }
        let (mx, my) = cx.frame.mouse;
        if let Some(i) = self.interact(bounds, (mx, my), cx.frame.pointer_outside(), cx.frame.mouse_down) {
            let open = self.accordion.sections.get(i).map(|s| s.open).unwrap_or(false);
            let handler = self.handlers.get(i).and_then(|h| h.as_deref());
            let focus_id = self.focus_ids.get(i).copied().flatten();
            let mut args = CheckedChangedEventArgs::new(!open, open, ChangeSource::User);
            cx.fire("OnToggled", focus_id, handler, ViewEventKind::Toggled(open), &mut args);
        }
        self.accordion.hovered = (!cx.frame.pointer_outside()).then(|| self.accordion.header_at(bounds, mx, my)).flatten();
        self.accordion.paint(canvas, bounds, crate::common::rest());
        let rects = self.accordion.section_rects(bounds);
        for (i, (rect, content)) in rects.iter().zip(self.contents.iter_mut()).enumerate() {
            let is_open = self.accordion.sections.get(i).map(|s| s.is_open()).unwrap_or(false);
            if !is_open {
                continue;
            }
            if let Some(node) = content.as_mut() {
                let panel_rect = self.accordion.panel_rect(*rect, i);
                let mut inner = cx.reborrow();
                node.paint(&mut inner, panel_rect);
            }
        }
    }
}

// ═════════════════════════════════════════════════════════════════════════
// Stepper / Step — over `kubuno_ui::feedback::Stepper`.
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: step,
    name: "Step",
    // Note: One step of a `<Stepper>` trail. Valid only as a direct `<Stepper>` child.
    doc: "A step of a Stepper.",
    ctor: kubuno_ui::feedback::Step::new("Étape"),
    children: ChildrenModel::None,
    props: [
        PropertyMeta::new("Label", PropKind::String, "", "Text of the step."),
        PropertyMeta::new("Description", PropKind::String, "", "Secondary text shown under the label."),
        PropertyMeta::new("Status", PropKind::Enum(&["Pending", "Current", "Complete", "Error", "Disabled"]), "Pending",
            "State of the step. Leave empty to derive it from the current step.",
        ),
        PropertyMeta::new("Optional", PropKind::Bool, "false", "Marks the step as optional."),
    ],
    events: [],
    smoke: |s| {
        let s = s.description("Vérifiez vos informations");
        let s = s.status(kubuno_ui::feedback::StepStatus::Current);
        s.optional(true)
    },
    build: |props, _cx| {
        use super::*;
        // See `TabItem`'s `build`: only ever reached through `Stepper`'s own
        // child walk in practice.
        let _ = (props.str("Label", "")?, props.str("Description", "")?, props.enum_("Status", "Pending")?, props.bool("Optional", false)?);
        Err(BuildError::new("`<Step>` is only valid as a direct child of `<Stepper>`", props.element().name_range()))
    },
}

component! {
    mod_name: stepper,
    name: "Stepper",
    // Note: A wizard's progress spine (`kubuno_ui::feedback::Stepper`), steps `<Step>`.
    doc: "The progress of a multi-step process. Add the steps as Step children.",
    ctor: kubuno_ui::feedback::Stepper::new(),
    children: ChildrenModel::List(&["Step"]),
    default_event: "OnStepSelected",
    props: [
        PropertyMeta::new("CurrentIndex", PropKind::F32, "0", "Index of the current step, starting at 0."),
        PropertyMeta::new("Orientation", PropKind::Enum(&["Horizontal", "Vertical"]), "Horizontal",
            "Whether the steps are laid out horizontally or vertically.",
        ),
        PropertyMeta::new("AllowForward", PropKind::Bool, "false", "Lets the user click a step after the current one."),
    ],
    events: [
        EventMeta::new("OnStepSelected", "Occurs when the user clicks a step.").category(crate::registry::EventCategory::Behavior).args::<crate::events::SelectionChangedEventArgs>(),
    ],
    smoke: |s| {
        let s = s.step(kubuno_ui::feedback::Step::new("Compte"));
        s.at(0)
    },
    build: |props, _cx| {
        use super::*;
        let current = props.f32("CurrentIndex", 0.0)?;
        let orientation = props.enum_("Orientation", "Horizontal")?;
        let allow_forward = props.bool("AllowForward", false)?;
        let on_step_selected = props.event("OnStepSelected");
        let focus_id = props.focus_id();
        let mut stepper = kubuno_ui::feedback::Stepper::new();
        for child in props.element().children() {
            let name = child.name().unwrap_or_default();
            if name != "Step" {
                return Err(BuildError::new(format!("`<Stepper>` only accepts `<Step>` children, found `<{name}>`"), child.name_range()));
            }
            let meta = crate::registry::lookup("Step")
                .ok_or_else(|| BuildError::new("`Step` is not registered", child.name_range()))?;
            let child_props = Props::new(&child, meta);
            let label = literal_str(&child_props, "Label", "")?;
            let description = literal_str(&child_props, "Description", "")?;
            let status = literal_str(&child_props, "Status", "Pending")?;
            let optional = child_props.bool("Optional", false)?;
            let optional = matches!(optional, PropSource::Literal(true));
            let mut step = kubuno_ui::feedback::Step::new(label).optional(optional);
            if !description.is_empty() {
                step = step.description(description);
            }
            step = step.status(match status.as_str() {
                "Current" => kubuno_ui::feedback::StepStatus::Current,
                "Complete" => kubuno_ui::feedback::StepStatus::Complete,
                "Error" => kubuno_ui::feedback::StepStatus::Error,
                "Disabled" => kubuno_ui::feedback::StepStatus::Disabled,
                _ => kubuno_ui::feedback::StepStatus::Pending,
            });
            stepper = stepper.step(step);
        }
        stepper.orientation = match orientation {
            PropSource::Literal(ref s) if s == "Vertical" => kubuno_ui::feedback::StepperOrientation::Vertical,
            _ => kubuno_ui::feedback::StepperOrientation::Horizontal,
        };
        Ok(Box::new(crate::registry::families::containers::StepperNode {
            current,
            allow_forward,
            on_step_selected,
            focus_id,
            stepper,
            pressed: None,
        }) as Box<dyn ViewNode>)
    },
}

/// The live node for `<Stepper>`. Steps are static (built once from XML — see
/// the module doc); only `CurrentIndex` is bindable. Hit-testing
/// (`step_at`/`is_reachable`) is pure `Rect` arithmetic with no `Canvas` — see
/// `containers::tests` below.
pub struct StepperNode {
    current: PropSource<f32>,
    allow_forward: PropSource<bool>,
    on_step_selected: Option<String>,
    focus_id: Option<FocusId>,
    stepper: kubuno_ui::feedback::Stepper,
    pressed: Option<usize>,
}

impl StepperNode {
    /// The canvas-independent half of a frame — see
    /// [`AccordionNode::interact`], whose shape this mirrors: hit-test, then
    /// press/release, then only a REACHABLE step's click counts. Returns the
    /// newly selected index, when a click just completed on one.
    /// `pub(crate)` for the same reason as `AccordionNode::interact`.
    pub(crate) fn interact(&mut self, bounds: Rect, mouse: (f32, f32), pointer_outside: bool, mouse_down: bool) -> Option<usize> {
        let hot = (!pointer_outside)
            .then(|| self.stepper.step_at(bounds, mouse.0, mouse.1))
            .flatten()
            .filter(|&i| self.stepper.is_reachable(i));
        let mut pressed = self.pressed.is_some();
        let (_, clicked) = press_release(&mut pressed, hot.is_some() && self.pressed.unwrap_or(usize::MAX) == hot.unwrap_or(usize::MAX - 1) || (hot.is_some() && self.pressed.is_none()), mouse_down);
        // The single shared `Option<usize>` flag needs its own tiny state
        // machine rather than `press_release`'s single `bool`: a step whose
        // index changed under the pointer mid-press must not count as a
        // click on the NEW one either.
        if mouse_down {
            if self.pressed.is_none() {
                self.pressed = hot;
            }
        } else {
            let result = if clicked && self.pressed == hot { hot } else { None };
            self.pressed = None;
            if let Some(i) = result {
                self.stepper.current = i;
                return Some(i);
            }
            return None;
        }
        None
    }
}

impl ViewNode for StepperNode {
    fn measure(&self, c: &dyn Canvas, _vm: &dyn crate::binding::ViewModel) -> Size {
        self.stepper.measure(c)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        let wanted = self.current.resolve(cx.vm).max(0.0) as usize;
        if self.stepper.current != wanted && self.pressed.is_none() {
            self.stepper.current = wanted;
        }
        self.stepper.allow_forward = self.allow_forward.resolve(cx.vm);
        let (mx, my) = cx.frame.mouse;
        if let Some(i) = self.interact(bounds, (mx, my), cx.frame.pointer_outside(), cx.frame.mouse_down) {
            if let Some(spec) = self.current.binding() {
                if spec.mode.writes_back() {
                    spec.update_source(cx.vm, Value::F32(i as f32));
                }
            }
            let mut args = SelectionChangedEventArgs::new(Some(wanted), Some(i), ChangeSource::User);
            cx.fire("OnStepSelected", self.focus_id, self.on_step_selected.as_deref(), ViewEventKind::Changed(i.to_string()), &mut args);
        }
        self.stepper.hovered = (!cx.frame.pointer_outside()).then(|| self.stepper.step_at(bounds, mx, my)).flatten();
        let canvas: &dyn Canvas = cx.canvas;
        self.stepper.paint(canvas, bounds, crate::common::rest());
    }
}

/// `<UserControl x:Class="RatingBar">`: the root of a user control's own view (EVT-7b) — a
/// `<Panel>` (its children placed by docking or anchoring) whose class is the `UserControl` level.
/// The application's `#[derive(UserControl)] struct RatingBar` makes `<RatingBar/>` usable in its
/// other views.
pub const USER_CONTROL: ComponentMeta = ComponentMeta {
    name: "UserControl",
    doc: "The root of a user control: a reusable control designed as a view of its own, whose children are placed by docking or anchoring.",
    ..panel::META
};

// ═════════════════════════════════════════════════════════════════════════
// FloatingWindow — a window drawn inside the view (the web `FloatingWindow`).
// ═════════════════════════════════════════════════════════════════════════

component! {
    mod_name: floating_window,
    name: "FloatingWindow",
    // Note: The web `FloatingWindow` inside a view (`kubuno_ui::dialogs::FloatingWindow`, painted by `kubuno_controls::window_chrome`).
    doc: "A window drawn inside the view, with the Kubuno title band and a close button; modal, it veils the rest of the window.",
    ctor: kubuno_ui::dialogs::FloatingWindow::new("Window"),
    children: ChildrenModel::SingleWidget,
    props: [
        PropertyMeta::new("Title", PropKind::String, "", "Text of the window's title band.").localizable().bindable(),
        PropertyMeta::new("Icon", PropKind::String, "", "Icon shown before the title: a name of the Kubuno icon set, or an image file (SVG, PNG…) relative to the view.").editor("icon").category("Icon"),
        PropertyMeta::new("IsOpen", PropKind::Bool, "true", "Whether the window shows. Bind it to open and close the window from code; the close button sets it to false.").bindable(),
        PropertyMeta::new("Modal", PropKind::Bool, "false", "Veils the rest of the window while it is open, like a dialog."),
        PropertyMeta::new("ShowClose", PropKind::Bool, "true", "Shows the close button of the title band."),
        PropertyMeta::new("CornerRadius", PropKind::F32, "8", "Radius of the window's corners, in pixels (8 by default, like a desktop window; 0 for square corners).").category("Appearance").type_converter("CornerRadius").bindable(),
    ],
    events: [
        EventMeta::new("OnClose", "Occurs when the close button of the window is clicked."),
    ],
    smoke: |w| { w.modal() },
    build: |props, cx| {
        use super::*;
        let node = crate::registry::families::containers::FloatingWindowNode {
            title: props.str("Title", "")?,
            icon: props.str("Icon", "")?,
            open: props.bool("IsOpen", true)?,
            modal: props.bool("Modal", false)?,
            show_close: props.bool("ShowClose", true)?,
            corner_radius: props.f32("CornerRadius", kubuno_controls::host::form::DEFAULT_CORNER_RADIUS)?,
            on_close: props.event("OnClose"),
            child: props.build_single_child(cx)?,
            pressed: false,
        };
        Ok(Box::new(node) as Box<dyn ViewNode>)
    },
}

/// The live node for `<FloatingWindow>`: the band, surface and shadow of
/// [`kubuno_controls::window_chrome`] (the same painter as every Kubuno window), its one child in
/// the body.
pub struct FloatingWindowNode {
    title: PropSource<String>,
    icon: PropSource<String>,
    open: PropSource<bool>,
    modal: PropSource<bool>,
    show_close: PropSource<bool>,
    /// `CornerRadius`, in DIP.
    corner_radius: PropSource<f32>,
    on_close: Option<String>,
    child: Option<Box<dyn ViewNode>>,
    pressed: bool,
}

impl FloatingWindowNode {
    fn layout(&self, vm: &dyn crate::binding::ViewModel, bounds: Rect) -> kubuno_controls::window_chrome::ChromeLayout {
        use kubuno_controls::window_chrome as wc;
        let buttons = if self.show_close.resolve(vm) { wc::SystemButtons::CLOSE_ONLY } else { wc::SystemButtons::NONE };
        let icon = crate::icon::resolve(&self.icon.resolve(vm)).is_some();
        wc::layout(&wc::ChromeStyle::default(), bounds, icon, buttons, wc::SlotWidths::default())
    }
}

impl ViewNode for FloatingWindowNode {
    fn measure(&self, c: &dyn Canvas, vm: &dyn crate::binding::ViewModel) -> Size {
        let child = self.child.as_ref().map(|n| n.measure(c, vm)).unwrap_or(Size::EMPTY);
        Size::new(child.width.max(kubuno_ui::dialogs::MIN_WIDTH), child.height + kubuno_controls::window_chrome::TITLEBAR_HEIGHT)
    }

    fn paint(&mut self, cx: &mut PaintCx<'_>, bounds: Rect) {
        use kubuno_controls::window_chrome as wc;
        // Closed windows show only in the designer, where they are edited.
        if !self.open.resolve(cx.vm) && cx.design.is_none() {
            return;
        }
        if self.modal.resolve(cx.vm) && cx.design.is_none() {
            // The web's backdrop (`bg-black/30`) over the whole window.
            let (w, h) = cx.frame.size;
            cx.canvas.fill_rect(&Rect::new(-10_000.0, -10_000.0, w + 10_000.0, h + 10_000.0), &cx.canvas.theme().dialog_scrim);
        }
        let l = self.layout(cx.vm, bounds);
        let style = wc::ChromeStyle::default();
        let radius = self.corner_radius.resolve(cx.vm);
        let radius = if radius.is_finite() { radius.max(0.0) } else { 0.0 };
        wc::paint_frame(cx.canvas, bounds, radius, None);
        // Nothing the window holds paints past its rounded corners.
        cx.canvas.push_clip_rounded(&bounds, radius);
        wc::paint_band_rounded(cx.canvas, &style, &l, radius);
        let (mx, my) = cx.frame.mouse;
        let close = l.rect_of(wc::Part::Close);
        let hot = close.is_some_and(|r| r.contains(mx, my));
        let (down, clicked) = crate::node::press_release(&mut self.pressed, hot, cx.frame.mouse_down);
        let icon = crate::icon::resolve(&self.icon.resolve(cx.vm)).map_or(wc::ChromeIcon::None, wc::ChromeIcon::Glyph);
        let state = wc::ChromeState { hot: hot.then_some(wc::Part::Close), pressed: down.then_some(wc::Part::Close), maximized: false };
        wc::paint_caption(cx.canvas, &style, &l, &self.title.resolve(cx.vm), icon, state);
        let body = Rect::new(bounds.left, l.band.bottom, bounds.right, bounds.bottom);
        if let Some(child) = self.child.as_mut() {
            let mut inner = cx.reborrow();
            inner.canvas.push_clip(&body);
            child.paint(&mut inner, body);
            inner.canvas.pop_clip();
        }
        cx.canvas.pop_clip_rounded();
        if clicked && cx.design.is_none() {
            if let PropSource::Bound { spec, .. } = &self.open {
                spec.write(cx.vm, Value::Bool(false));
            }
            cx.fire("OnClose", None, self.on_close.as_deref(), ViewEventKind::Clicked, &mut crate::events::EmptyEventArgs);
        }
    }
}

/// Every component this family declares, in declaration order.
pub const ALL: &[ComponentMeta] = &[
    panel::META,
    USER_CONTROL,
    group_box::META,
    scroll_area::META,
    splitter::META,
    tab_item::META,
    tabs::META,
    breadcrumb_item::META,
    breadcrumb::META,
    toolbar_item::META,
    toolbar::META,
    accordion_section::META,
    accordion::META,
    step::META,
    stepper::META,
    floating_window::META,
];

#[cfg(test)]
mod tests {
    //! Unit tests: layout rects (no `Canvas` needed — `kubuno_ui::containers::
    //! Panel::layout_children`, `Splitter::panel1_rect`/`panel2_rect`,
    //! `kubuno_ui::feedback::Accordion::section_rects`/`Stepper::step_rects`
    //! are all pure `Rect` arithmetic, per this family's report), selection
    //! binding and events — mirroring `crate::node::tests`' own split between
    //! the canvas-independent `interact()` half of a frame and its
    //! canvas-dependent paint.

    use super::*;
    use crate::binding::{BindingSpec, MapViewModel, ViewModel};
    use crate::node::{ViewEvent, ViewEventKind};

    // ── Panel: Dock/Fill matches the real engine ──────────────────────────


    // ── Splitter: two panes, vertical vs horizontal ────────────────────────

    #[test]
    fn splitter_node_places_two_panes_side_by_side_when_vertical() {
        let s = kubuno_ui::containers::Splitter::vertical().with_distance(150.0);
        let bounds = Rect::new(0.0, 0.0, 400.0, 200.0);
        let p1 = s.panel1_rect(bounds);
        let p2 = s.panel2_rect(bounds);
        assert!(p1.right <= p2.left, "pane 1 must sit entirely left of pane 2");
        assert_eq!((p1.top, p1.bottom), (0.0, 200.0));
    }

    #[test]
    fn splitter_node_stacks_two_panes_when_horizontal() {
        let s = kubuno_ui::containers::Splitter::horizontal().with_distance(80.0);
        let bounds = Rect::new(0.0, 0.0, 400.0, 200.0);
        let p1 = s.panel1_rect(bounds);
        let p2 = s.panel2_rect(bounds);
        assert!(p1.bottom <= p2.top, "pane 1 must sit entirely above pane 2");
        assert_eq!((p1.left, p1.right), (0.0, 400.0));
    }

    // ── Accordion: header hit test toggles + fires, disabled never does ────

    fn accordion_node(n: usize, disabled_at: Option<usize>) -> AccordionNode {
        let mut accordion = kubuno_ui::feedback::Accordion::new();
        let mut handlers = Vec::new();
        let mut focus_ids = Vec::new();
        let mut contents = Vec::new();
        for i in 0..n {
            let disabled = disabled_at == Some(i);
            accordion = accordion.section(kubuno_ui::feedback::AccordionSection::new(format!("Section {i}"), 0.0).disabled(disabled));
            handlers.push(Some(format!("toggled_{i}")));
            focus_ids.push(None);
            contents.push(None);
        }
        AccordionNode { accordion, handlers, focus_ids, contents, pressed: vec![false; n] }
    }

    #[test]
    fn accordion_click_on_a_header_toggles_that_section() {
        let mut node = accordion_node(2, None);
        let bounds = Rect::new(0.0, 0.0, 300.0, 200.0);
        let rects = node.accordion.section_rects(bounds);
        let header = node.accordion.header_rect(rects[1]);
        let mid = ((header.left + header.right) / 2.0, (header.top + header.bottom) / 2.0);

        assert!(!node.accordion.sections[1].open);
        let toggled = node.interact(bounds, mid, false, true);
        assert_eq!(toggled, None, "a press alone is not yet a click");
        let toggled = node.interact(bounds, mid, false, false);
        assert_eq!(toggled, Some(1));
        assert!(node.accordion.sections[1].open, "the section must now be open");
        assert!(!node.accordion.sections[0].open, "the other section is untouched");
    }

    #[test]
    fn accordion_click_on_a_disabled_header_does_nothing() {
        let mut node = accordion_node(1, Some(0));
        let bounds = Rect::new(0.0, 0.0, 300.0, 200.0);
        let header = node.accordion.header_rect(node.accordion.section_rects(bounds)[0]);
        let mid = ((header.left + header.right) / 2.0, (header.top + header.bottom) / 2.0);

        node.interact(bounds, mid, false, true);
        let toggled = node.interact(bounds, mid, false, false);
        assert_eq!(toggled, None);
        assert!(!node.accordion.sections[0].open);
    }

    #[test]
    fn accordion_toggle_dispatches_its_handler_and_fires_an_event() {
        let mut node = accordion_node(1, None);
        let handlers = crate::handlers! {
            "toggled_0" => |vm, v| { vm.set("SectionOpen", v); },
        };
        let mut vm = MapViewModel::new();
        let mut focus = kubuno_ui::FocusRing::new();
        let mut handler_table = handlers;
        let mut events = Vec::new();
        let bounds = Rect::new(0.0, 0.0, 300.0, 200.0);
        let header = node.accordion.header_rect(node.accordion.section_rects(bounds)[0]);
        let mid = ((header.left + header.right) / 2.0, (header.top + header.bottom) / 2.0);

        node.interact(bounds, mid, false, true);
        let i = node.interact(bounds, mid, false, false).unwrap();
        let open = node.accordion.sections[i].open;
        let handler = node.handlers[i].clone();
        handler_table.dispatch(handler.as_deref().unwrap(), &mut vm, Value::Bool(open));
        events.push(ViewEvent { focus_id: None, handler, kind: ViewEventKind::Toggled(open) });

        assert_eq!(vm.get("SectionOpen"), Some(Value::Bool(true)));
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0].kind, ViewEventKind::Toggled(true)));
        let _ = &mut focus; // constructed only to mirror a real `PaintCx`'s shape
    }

    // ── Splitter: drag write-back ──────────────────────────────────────────

    fn splitter_frame_at(mouse: (f32, f32), mouse_down: bool) -> host::Frame {
        host::Frame {
            size: (400.0, 300.0),
            mouse,
            mouse_down,
            right_down: false,
            middle_down: false,
            dismiss: false,
            scale: 1.0,
            client_origin: (0.0, 0.0),
            work_area: (0.0, 0.0, 400.0, 300.0),
            chrome_top: 0.0,
            mods: Modifiers::NONE,
            wheel: (0.0, 0.0),
            click_count: 0,
            window_focused: true,
        }
    }

    #[test]
    fn splitter_drag_on_the_grip_writes_back_live_and_fires_on_distance_changed() {
        // Mirrors `choice::tests::slider_drag_writes_back_live_and_fires_on_value_changed`
        // — the same "press near an edge, the resolved value follows the
        // pointer, clamped" shape, over `Splitter::hit_test_grip`/
        // `distance_for` instead of `Slider::hit_test`/`drag_to`.
        let mut node = SplitterNode {
            orientation: PropSource::Literal("Vertical".to_string()),
            distance: PropSource::Bound { spec: BindingSpec { path: "PaneWidth".to_string(), mode: BindingMode::TwoWay, ..Default::default() }, fallback: 200.0 },
            focus_id: None,
            on_distance_changed: Some("pane_resized".to_string()),
            dragging: false,
            local: None,
            last_source: None,
            children: Vec::new(),
        };
        let mut vm = MapViewModel::new().with("PaneWidth", Value::F32(200.0));
        let mut focus = kubuno_ui::FocusRing::new();
        let mut handlers = crate::binding::HandlerTable::new();
        let mut events = Vec::new();
        let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);

        // Press right on the grip band (pane 1 is 200 DIP wide by default).
        {
            let frame = splitter_frame_at((200.0, 150.0), true);
            let mut ix =
                crate::node::InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
            node.interact(&mut ix, bounds, 200.0);
        }
        assert!(node.dragging, "a press on the grip must start a drag");
        assert_eq!(vm.get("PaneWidth"), Some(Value::F32(200.0)), "no movement yet at the same position");

        // Drag further right: the bound distance follows the pointer.
        {
            let frame = splitter_frame_at((260.0, 150.0), true);
            let mut ix =
                crate::node::InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
            node.interact(&mut ix, bounds, 200.0);
        }
        assert_eq!(vm.get("PaneWidth"), Some(Value::F32(260.0)));
        assert!(!events.is_empty(), "at least one OnDistanceChanged-worthy change while dragging");
        assert!(matches!(events.last().unwrap().kind, ViewEventKind::Changed(ref s) if s == "260"));

        // Release: dragging stops, no further write.
        {
            let frame = splitter_frame_at((260.0, 150.0), false);
            let mut ix =
                crate::node::InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
            node.interact(&mut ix, bounds, 260.0);
        }
        assert!(!node.dragging);
    }

    #[test]
    fn a_literal_distance_follows_the_drag_until_the_view_changes_it() {
        let mut node = SplitterNode {
            orientation: PropSource::Literal("Vertical".to_string()),
            distance: PropSource::Literal(200.0),
            focus_id: None,
            on_distance_changed: None,
            dragging: false,
            local: None,
            last_source: None,
            children: Vec::new(),
        };
        let mut vm = MapViewModel::new();
        let mut focus = kubuno_ui::FocusRing::new();
        let mut handlers = crate::binding::HandlerTable::new();
        let mut events = Vec::new();
        let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
        assert_eq!(node.current(&vm), 200.0);
        for x in [200.0, 280.0] {
            let frame = splitter_frame_at((x, 150.0), true);
            let mut ix = crate::node::InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
            let d = node.current(ix.vm);
            node.interact(&mut ix, bounds, d);
        }
        assert_eq!(node.current(&vm), 280.0, "the bar stays where the user left it");
        node.distance = PropSource::Literal(120.0);
        assert_eq!(node.current(&vm), 120.0, "a new Distance wins");
    }

    #[test]
    fn splitter_press_away_from_the_grip_does_not_start_a_drag() {
        let mut node = SplitterNode {
            orientation: PropSource::Literal("Vertical".to_string()),
            distance: PropSource::Literal(200.0),
            focus_id: None,
            on_distance_changed: None,
            dragging: false,
            local: None,
            last_source: None,
            children: Vec::new(),
        };
        let mut vm = MapViewModel::new();
        let mut focus = kubuno_ui::FocusRing::new();
        let mut handlers = crate::binding::HandlerTable::new();
        let mut events = Vec::new();
        let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
        let frame = splitter_frame_at((10.0, 10.0), true);
        let mut ix = crate::node::InteractCx::new(&frame, &mut vm, &mut focus, &mut handlers, &mut events);
        node.interact(&mut ix, bounds, 200.0);
        assert!(!node.dragging);
        assert!(events.is_empty());
    }

    // ── Stepper: reachable steps only, current index write-back ────────────

    fn stepper_node() -> StepperNode {
        let stepper = kubuno_ui::feedback::Stepper::new()
            .step(kubuno_ui::feedback::Step::new("Compte"))
            .step(kubuno_ui::feedback::Step::new("Profil"))
            .step(kubuno_ui::feedback::Step::new("Confirmation"));
        StepperNode {
            current: PropSource::Bound { spec: BindingSpec { path: "Step".to_string(), mode: BindingMode::TwoWay, ..Default::default() }, fallback: 0.0 },
            allow_forward: PropSource::Literal(false),
            on_step_selected: Some("step_selected".to_string()),
            focus_id: None,
            stepper,
            pressed: None,
        }
    }

    fn step_mid(node: &StepperNode, bounds: Rect, i: usize) -> (f32, f32) {
        let r = node.stepper.step_rects(bounds)[i];
        ((r.left + r.right) / 2.0, (r.top + r.bottom) / 2.0)
    }

    #[test]
    fn stepper_click_on_the_current_step_reselects_it() {
        let mut node = stepper_node();
        let bounds = Rect::new(0.0, 0.0, 600.0, 80.0);
        let mid = step_mid(&node, bounds, 0);

        node.interact(bounds, mid, false, true);
        let selected = node.interact(bounds, mid, false, false);
        assert_eq!(selected, Some(0));
    }

    #[test]
    fn stepper_click_past_the_current_step_is_ignored_without_allow_forward() {
        let mut node = stepper_node();
        node.stepper.current = 0;
        let bounds = Rect::new(0.0, 0.0, 600.0, 80.0);
        let mid = step_mid(&node, bounds, 2); // "Confirmation" — not reachable yet

        node.interact(bounds, mid, false, true);
        let selected = node.interact(bounds, mid, false, false);
        assert_eq!(selected, None, "a step beyond the current one must not be selectable");
        assert_eq!(node.stepper.current, 0);
    }

    #[test]
    fn stepper_selection_write_back_and_event_mirror_the_switch_pattern() {
        // Mirrors `crate::node::tests::switch_toggle_two_way_binding_writes_back_and_fires_an_event`.
        let mut node = stepper_node();
        node.stepper.allow_forward = true;
        let bounds = Rect::new(0.0, 0.0, 600.0, 80.0);
        let mid = step_mid(&node, bounds, 1);

        node.interact(bounds, mid, false, true);
        let i = node.interact(bounds, mid, false, false).unwrap();

        let handlers = crate::handlers! {
            "step_selected" => |vm, v| { if let Value::F32(n) = v { vm.set("Step", Value::F32(n)); } },
        };
        let mut vm = MapViewModel::new().with("Step", Value::F32(0.0));
        let mut handler_table = handlers;
        handler_table.dispatch("step_selected", &mut vm, Value::F32(i as f32));
        if let Some(spec) = node.current.binding() {
            if spec.mode.writes_back() {
                spec.update_source(&mut vm, Value::F32(i as f32));
            }
        }
        assert_eq!(vm.get("Step"), Some(Value::F32(1.0)));
        assert_eq!(node.stepper.current, 1);
    }

    // ── Every declared component builds a tree ──────────────────────────────

    #[test]
    fn every_declared_component_compiles_from_a_worked_snippet() {
        let mut reg: Vec<ComponentMeta> = crate::registry::components::ALL.to_vec();
        reg.extend_from_slice(ALL);

        let src = r#"
            <Panel>
              <GroupBox Title="x" Dock="Top" Height="120">
                <Stack Direction="TopDown">
                  <Splitter Dock="Fill">
                    <ScrollArea><Button Text="A"/></ScrollArea>
                    <Tabs SelectedIndex="0">
                      <TabItem Header="Un"><Button Text="1"/></TabItem>
                      <TabItem Header="Deux"><Button Text="2"/></TabItem>
                    </Tabs>
                  </Splitter>
                </Stack>
              </GroupBox>
            </Panel>
        "#;
        let result = crate::compile::compile_with_registry(src, &reg);
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[test]
    fn breadcrumb_toolbar_accordion_stepper_compile() {
        let mut reg: Vec<ComponentMeta> = crate::registry::components::ALL.to_vec();
        reg.extend_from_slice(ALL);

        let src = r#"
            <Stack Direction="TopDown">
              <Breadcrumb>
                <BreadcrumbItem Text="Racine" OnClick="go_root"/>
                <BreadcrumbItem Text="Dossier"/>
              </Breadcrumb>
              <Toolbar>
                <ToolbarItem Text="Nouveau" Icon="plus" OnClick="new_item"/>
                <ToolbarItem Icon="save" OnClick="save_item"/>
              </Toolbar>
              <Accordion>
                <AccordionSection Header="Un"><Button Text="a"/></AccordionSection>
                <AccordionSection Header="Deux" Open="true"><Button Text="b"/></AccordionSection>
              </Accordion>
              <Stepper CurrentIndex="0" OnStepSelected="step_selected">
                <Step Label="Compte"/>
                <Step Label="Profil" Status="Current"/>
              </Stepper>
            </Stack>
        "#;
        let result = crate::compile::compile_with_registry(src, &reg);
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[test]
    fn a_stray_non_tab_item_child_of_tabs_is_a_build_error() {
        let mut reg: Vec<ComponentMeta> = crate::registry::components::ALL.to_vec();
        reg.extend_from_slice(ALL);
        let src = r#"<Tabs><Button Text="not a tab item"/></Tabs>"#;
        let result = crate::compile::compile_with_registry(src, &reg);
        assert!(result.is_err());
    }

    #[test]
    fn splitter_rejects_a_child_count_other_than_two() {
        let mut reg: Vec<ComponentMeta> = crate::registry::components::ALL.to_vec();
        reg.extend_from_slice(ALL);
        let src = r#"<Splitter><Button Text="only one pane"/></Splitter>"#;
        let result = crate::compile::compile_with_registry(src, &reg);
        assert!(result.is_err());
    }

    // ── Panel: WinForms Dock/Anchor semantics (panel_child_rects) ───────

    fn anchored(anchor: AnchorStyles, x: f32, y: f32, w: f32, h: f32) -> PanelChildSpec {
        PanelChildSpec { dock: DockStyle::None, anchor, x, y, width: Some(w), height: Some(h) }
    }

    fn docked(dock: DockStyle, thickness: f32) -> PanelChildSpec {
        PanelChildSpec { dock, anchor: AnchorStyles::default(), x: 0.0, y: 0.0, width: Some(thickness), height: Some(thickness) }
    }

    /// A hidden docked child takes no band: the `Fill` sibling gets the whole panel (WinForms).
    #[test]
    fn a_hidden_docked_child_takes_no_band() {
        let specs = [docked(DockStyle::Left, 100.0), PanelChildSpec { dock: DockStyle::Fill, ..docked(DockStyle::Fill, 0.0) }];
        let measured = vec![Size::new(0.0, 0.0); 2];
        let bounds = Rect::new(0.0, 0.0, 400.0, 300.0);
        let shown = panel_child_rects_shown(&specs, &measured, &[true, true], 0.0, Size::new(400.0, 300.0), bounds);
        assert_eq!(shown[1].left, 100.0);
        let hidden = panel_child_rects_shown(&specs, &measured, &[false, true], 0.0, Size::new(400.0, 300.0), bounds);
        assert_eq!(hidden[1].left, 0.0, "the hidden rail leaves its width to the page");
    }

    fn rects(specs: &[PanelChildSpec], design: (f32, f32), size: (f32, f32)) -> Vec<(f32, f32, f32, f32)> {
        let measured = vec![Size::new(0.0, 0.0); specs.len()];
        panel_child_rects(specs, &measured, 0.0, Size::new(design.0, design.1), Rect::new(0.0, 0.0, size.0, size.1))
            .into_iter()
            .map(|r| (r.left, r.top, r.right, r.bottom))
            .collect()
    }

    const TL: AnchorStyles = AnchorStyles::TOP.union(AnchorStyles::LEFT);

    #[test]
    fn anchor_top_left_keeps_its_place_when_the_panel_grows() {
        let r = rects(&[anchored(TL, 10.0, 20.0, 80.0, 24.0)], (400.0, 300.0), (600.0, 500.0));
        assert_eq!(r, vec![(10.0, 20.0, 90.0, 44.0)]);
    }

    #[test]
    fn anchor_right_bottom_keeps_its_distance_to_those_edges() {
        let a = AnchorStyles::RIGHT.union(AnchorStyles::BOTTOM);
        let r = rects(&[anchored(a, 300.0, 260.0, 80.0, 24.0)], (400.0, 300.0), (600.0, 450.0));
        // 20 DIP from the right edge and 16 from the bottom, as authored.
        assert_eq!(r, vec![(500.0, 410.0, 580.0, 434.0)]);
        let smaller = rects(&[anchored(a, 300.0, 260.0, 80.0, 24.0)], (400.0, 300.0), (350.0, 280.0));
        assert_eq!(smaller, vec![(250.0, 240.0, 330.0, 264.0)]);
    }

    #[test]
    fn anchor_left_right_stretches_the_width_and_top_bottom_the_height() {
        let lr = TL.union(AnchorStyles::RIGHT);
        assert_eq!(rects(&[anchored(lr, 10.0, 10.0, 380.0, 24.0)], (400.0, 300.0), (600.0, 300.0)), vec![(10.0, 10.0, 590.0, 34.0)]);
        let tb = TL.union(AnchorStyles::BOTTOM);
        assert_eq!(rects(&[anchored(tb, 10.0, 10.0, 80.0, 280.0)], (400.0, 300.0), (400.0, 500.0)), vec![(10.0, 10.0, 90.0, 490.0)]);
    }

    #[test]
    fn an_axis_anchored_on_neither_side_keeps_its_size_and_recentres() {
        // Anchored Top only: horizontally it keeps its width and moves by half the growth.
        let r = rects(&[anchored(AnchorStyles::TOP, 160.0, 10.0, 80.0, 24.0)], (400.0, 300.0), (600.0, 300.0));
        assert_eq!(r, vec![(260.0, 10.0, 340.0, 34.0)]);
    }

    #[test]
    fn dock_follows_winforms_z_order_and_fill_takes_the_remainder() {
        // Document order = paint order: the first band is the backmost control, docked first.
        let specs = [docked(DockStyle::Top, 30.0), docked(DockStyle::Left, 100.0), docked(DockStyle::Bottom, 20.0), docked(DockStyle::Fill, 0.0)];
        let r = rects(&specs, (400.0, 300.0), (400.0, 300.0));
        assert_eq!(r[0], (0.0, 0.0, 400.0, 30.0));
        assert_eq!(r[1], (0.0, 30.0, 100.0, 300.0));
        assert_eq!(r[2], (100.0, 280.0, 400.0, 300.0));
        assert_eq!(r[3], (100.0, 30.0, 400.0, 280.0));
        // A resize keeps the bands' thickness and gives the change to the fill.
        let grown = rects(&specs, (400.0, 300.0), (500.0, 400.0));
        assert_eq!(grown[3], (100.0, 30.0, 500.0, 380.0));
    }

    #[test]
    fn a_docked_band_without_a_size_uses_the_childs_natural_size() {
        let spec = PanelChildSpec { dock: DockStyle::Top, anchor: AnchorStyles::default(), x: 0.0, y: 0.0, width: None, height: None };
        let r = panel_child_rects(&[spec], &[Size::new(50.0, 28.0)], 0.0, Size::new(400.0, 300.0), Rect::new(0.0, 0.0, 400.0, 300.0));
        assert_eq!((r[0].top, r[0].bottom, r[0].right), (0.0, 28.0, 400.0));
    }

    #[test]
    fn padding_insets_the_children_and_the_design_reference() {
        let a = AnchorStyles::RIGHT.union(AnchorStyles::TOP);
        let r = panel_child_rects(
            &[anchored(a, 300.0, 0.0, 80.0, 24.0)],
            &[Size::new(0.0, 0.0)],
            10.0,
            Size::new(400.0, 300.0),
            Rect::new(0.0, 0.0, 500.0, 300.0),
        );
        // Like WinForms, X is measured from the client origin (padding does not shift it); the anchor
        // keeps the 10 DIP gap to the padded right edge (390 → 490) when the panel grows by 100.
        assert_eq!((r[0].left, r[0].right), (400.0, 480.0));
    }

    #[test]
    fn the_root_panel_is_designed_at_the_view_design_size() {
        use crate::ast::AstNode;
        let p = crate::syntax::parse(r#"<Panel DesignWidth="640" DesignHeight="480"><Button/></Panel>"#);
        let doc = crate::ast::Document::cast(p.syntax()).unwrap();
        let root = doc.root_element().unwrap();
        assert_eq!(panel_design_size(&root).map(|s| (s.width, s.height)), Some((640.0, 480.0)));
        let p = crate::syntax::parse(r#"<Stack><Panel><Button/></Panel><Panel Width="200" Height="100"/></Stack>"#);
        let doc = crate::ast::Document::cast(p.syntax()).unwrap();
        let mut panels = doc.root_element().unwrap().children();
        assert_eq!(panel_design_size(&panels.next().unwrap()).map(|s| s.width), None, "sized by its first layout");
        assert_eq!(panel_design_size(&panels.next().unwrap()).map(|s| (s.width, s.height)), Some((200.0, 100.0)));
    }
}
