//! Virtual regions (`vskubuno/docs/DESKTOP-MIGRATION.md`, lot F3): a node that lays out its own
//! sub-elements — a `<Ribbon>`'s tabs, groups and buttons — declares, every frame, `(element id,
//! rectangle, component)` for each of them. The designer then records them in its layout map (they
//! are selected, outlined and dropped into like any element), and the input router routes the
//! pointer to them (MouseEnter/Leave, MouseDown, hover, tooltips) and raises their events through
//! their own class instance, with their own `Sender`.
//!
//! A sub-element is an ordinary element of the `.kbview` (its id is its stable id), so its
//! attributes are read like any element's — a `{Binding …}` included — and a named one is a named
//! component of the view (`self.bold` in the code-behind).
//!
//! Two design-time helpers live here too: the designer's current selection (what a ribbon shows —
//! the selected tab is the active one) and the late paint of what a node draws over its siblings
//! in the designer (a ribbon's inline drop-down).

use std::cell::RefCell;
use std::rc::Rc;

use kubuno_controls::ControlCanvas;
use kubuno_ui::{FocusId, Rect};

use crate::ast::Element;
use crate::component::Component;
use crate::design::{parent_id_of, LayoutEntry, LayoutMap};
use crate::events::router::SlotEvents;
use crate::events::EventArgs;
use crate::node::{PaintCx, ViewEventKind};
use crate::props::{BuildCx, BuildError, Props};
use crate::registry::{self, ChildrenModel, ComponentMeta, LayoutKind};

/// One sub-element a node lays out itself.
pub struct VirtualElement {
    /// Its stable id (`"0.1.2"`).
    pub id: String,
    /// Its `x:Name`.
    pub name: Option<String>,
    pub meta: &'static ComponentMeta,
    /// The element as the router sees it (its `On*` handlers).
    pub slot: Rc<SlotEvents>,
    /// Its class instance (a `RibbonButton`…).
    pub control: Option<Rc<RefCell<dyn Component>>>,
    /// It accepts children (a drop target in the designer).
    pub container: bool,
}

impl VirtualElement {
    /// Reads `element`: its id, handlers and class instance; a named one joins the view's named
    /// components (DATA-2).
    pub fn build(element: &Element, cx: &mut BuildCx) -> Result<Self, BuildError> {
        Self::build_with(element, cx, true)
    }

    /// [`Self::build`]; `register`: a named element joins the view's named components (not for an
    /// element another node already registers, such as a `<Command>` a ribbon reads).
    pub fn build_with(element: &Element, cx: &mut BuildCx, register: bool) -> Result<Self, BuildError> {
        let name = element.name().ok_or_else(|| BuildError::new("element has no name", element.name_range()))?;
        let meta = registry::lookup(&name).ok_or_else(|| BuildError::new(format!("unknown element `<{name}>`"), element.name_range()))?;
        let slot = Rc::new(SlotEvents::from_element(element, meta, false));
        let x_name = element.attribute("x:Name").and_then(|a| a.value()).filter(|n| !n.is_empty());
        let control = crate::controls::class_of(meta.name).and_then(|class| (class.create)());
        if let (Some(instance), Some(n), true) = (&control, &x_name, register) {
            if let Ok(mut c) = instance.try_borrow_mut() {
                c.set_site(Some(crate::component::Site { name: n.clone(), design_mode: false, container: None }));
            }
            cx.components.push(crate::scope::Entry { name: n.clone(), class: meta.name, instance: Rc::downgrade(instance), slot: slot.clone() });
        }
        Ok(Self { id: slot.id.clone(), name: x_name, meta, slot, control, container: meta.children != ChildrenModel::None })
    }

    /// The typed reading of its attributes.
    pub fn props<'a>(&self, element: &'a Element) -> Props<'a> {
        Props::new(element, self.meta)
    }

    pub fn focus_id(&self) -> Option<FocusId> {
        self.slot.focus_id
    }

    /// Declares where the element was drawn this frame: to the designer's layout map and to the
    /// input router.
    pub fn report(&self, cx: &mut PaintCx<'_>, rect: Rect, enabled: bool) {
        if let Some(map) = cx.design.as_mut() {
            map.push(LayoutEntry { id: self.id.clone(), parent_id: parent_id_of(&self.id), bounds: rect, layout: LayoutKind::None, container: self.container, locked: false, clip: None });
        }
        if let Some(router) = cx.router.as_deref_mut() {
            router.register_with_control(self.slot.clone(), rect, self.control.as_ref());
            if !enabled {
                router.disable_last();
            }
        }
        if let Some(control) = self.control.as_ref() {
            if let Ok(mut c) = control.try_borrow_mut() {
                let design = cx.design.is_some();
                if c.site().is_none_or(|s| s.design_mode != design) {
                    let name = self.name.clone().unwrap_or_default();
                    c.set_site(Some(crate::component::Site { name, design_mode: design, container: None }));
                }
                if let Some(ctl) = c.as_control_mut() {
                    let core = ctl.control_core_mut();
                    core.bounds = rect;
                    core.props.enabled = enabled;
                }
            }
        }
    }

    /// Raises one of the element's own events (`OnClick`…) through its class instance, the element
    /// being the sender, like `PaintCx::fire` does for a node's own element.
    pub fn fire(&self, cx: &mut PaintCx<'_>, rect: Rect, event: &'static str, kind: ViewEventKind, args: &mut dyn EventArgs) {
        let handler = self.slot.handler(event).map(str::to_string);
        let cell = self.control.clone();
        let mut guard = cell.as_ref().and_then(|c| c.try_borrow_mut().ok());
        let mut inner = cx.reborrow();
        inner.sender = Some((self.slot.clone(), rect));
        inner.control = guard.as_deref_mut().and_then(|c| c.as_control_mut());
        inner.fire(event, self.slot.focus_id, handler.as_deref(), kind, args);
    }
}

/// Anything painted late, with its layout entries.
type LatePaint = Box<dyn FnOnce(&dyn ControlCanvas)>;
/// Anything painted late, with its layout entries.
type Late = (Vec<LayoutEntry>, LatePaint);

thread_local! {
    static SELECTION: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    static LATE: RefCell<Vec<Late>> = const { RefCell::new(Vec::new()) };
    static DESIGN_CAPTION: std::cell::Cell<Option<Caption>> = const { std::cell::Cell::new(None) };
}

/// The designer's selection (element ids), set by the design surface before each frame.
pub fn set_design_selection(ids: &[String]) {
    SELECTION.with(|s| {
        let mut s = s.borrow_mut();
        s.clear();
        s.extend(ids.iter().cloned());
    });
}

/// The designer's selection, as last set.
pub fn design_selection() -> Vec<String> {
    SELECTION.with(|s| s.borrow().clone())
}

/// Paints `paint` and records `entries` after the whole view (in the designer): what a node draws
/// over its siblings, such as a ribbon's inline drop-down.
pub fn defer_late(entries: Vec<LayoutEntry>, paint: LatePaint) {
    LATE.with(|l| l.borrow_mut().push((entries, paint)));
}

/// Runs what was deferred this frame (called by the runtime after the view painted).
pub(crate) fn flush_late(canvas: &dyn ControlCanvas, mut map: Option<&mut LayoutMap>) {
    let late = LATE.with(|l| std::mem::take(&mut *l.borrow_mut()));
    for (entries, paint) in late {
        paint(canvas);
        if let Some(map) = map.as_deref_mut() {
            for e in entries {
                map.push(e);
            }
        }
    }
}

/// A clickable glyph a node draws on the design surface (a ribbon's « + » and its smart tag): a
/// click on it selects `element_id` and asks the designer for the menu `menu` (`"add"`: what can be
/// added into the element; `"tasks"`: the element's tasks).
#[derive(Debug, Clone, PartialEq)]
pub struct DesignGlyph {
    pub rect: Rect,
    pub element_id: String,
    pub menu: &'static str,
}

thread_local! {
    static DESIGN_GLYPHS: RefCell<Vec<DesignGlyph>> = const { RefCell::new(Vec::new()) };
}

/// Declares a design glyph drawn this frame (see [`DesignGlyph`]).
pub fn push_design_glyph(glyph: DesignGlyph) {
    DESIGN_GLYPHS.with(|g| g.borrow_mut().push(glyph));
}

/// The design glyphs of the frame that just painted, taken (the design surface hit-tests them
/// before its own selection).
pub fn take_design_glyphs() -> Vec<DesignGlyph> {
    DESIGN_GLYPHS.with(|g| std::mem::take(&mut *g.borrow_mut()))
}

/// Paints a smart-tag glyph (Windows Forms' small square with a ▸) at `rect`.
pub fn paint_smart_tag(c: &dyn ControlCanvas, rect: Rect) {
    c.fill_rect(&rect, &kubuno_ui::ribbon::hex(0xFFFFFF));
    c.stroke_rect(&rect, &kubuno_ui::ribbon::hex(0x1E1E1E));
    c.text("▸", &rect, &c.formats().caption, &kubuno_ui::ribbon::hex(0x1E1E1E), true);
}

/// A caption's band and ink colours.
pub type Caption = (windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F, windows::Win32::Graphics::Direct2D::Common::D2D1_COLOR_F);

/// The colours the designed window's caption takes this frame (a ribbon's tab strip, which the
/// title bar continues — what kubuno_controls::host::set_caption_colors does at run time).
pub fn set_design_caption(colors: Caption) {
    DESIGN_CAPTION.with(|c| c.set(Some(colors)));
}

/// The design caption colours of the frame that just painted, taken.
pub fn take_design_caption() -> Option<Caption> {
    DESIGN_CAPTION.with(|c| c.take())
}

/// crate::design::paint_view_caption with the band and ink of colors: the designed window's
/// title band repainted in the ribbon's colour, then its icon, title and inert caption buttons.
pub fn paint_design_caption(c: &dyn ControlCanvas, frame: &crate::design::FrameLayout, style: &crate::design::ViewFrameStyle, colors: Caption) {
    use kubuno_controls::window_chrome as wc;
    let theme = c.theme();
    let form = style.form(theme);
    if let Some(chrome) = style.design_chrome(theme, frame) {
        let mut s = chrome.style.clone();
        s.background = Some(colors.0);
        s.foreground = Some(colors.1);
        let band = wc::layout(&s, chrome.bounds, chrome.has_icon, chrome.buttons, Default::default());
        wc::paint_band_rounded(c, &s, &band, form.corner.radius());
        let l = wc::layout(&s, chrome.bounds, chrome.has_icon, chrome.buttons, crate::window::declared_slots());
        // A glyph name or an image file (drawn by `kubuno_controls::icon_image`).
        let glyph = form.icon.as_deref().and_then(crate::icon::resolve);
        let icon = match glyph {
            Some(g) => wc::ChromeIcon::Glyph(g),
            None if form.icon.is_some() => wc::ChromeIcon::Glyph("AppWindow"),
            None => wc::ChromeIcon::None,
        };
        let title = if style.title.is_empty() { "Form" } else { style.title.as_str() };
        wc::paint_caption(c, &s, &l, title, icon, wc::ChromeState::default());
    }
}