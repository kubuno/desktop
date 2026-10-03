//! [`Props`] — the typed reading of one element's attributes that
//! [`crate::registry`]'s `component!` table drives, per the orchestrator's
//! architecture decision: "the table lists properties once; the macro
//! produces both the descriptive metadata (`PROPERTIES`, read by
//! [`crate::validate`]) and the typed reading of each property through
//! `Props` accessors". A `build` closure calls `props.bool("On", false)`,
//! `props.f32("Gap", 8.0)`, `props.str("Text", "")`, `props.enum_("Variant",
//! "Primary")` — one accessor per [`crate::registry::PropKind`] — and gets
//! back a [`crate::binding::PropSource`] rather than a bare value: a literal
//! is resolved once at build time, a `{Binding …}` expression carries its
//! path through unresolved, to be read fresh every frame (§5).
//!
//! No `Box<dyn Any>` and no second, hand-written setter list: an accessor
//! looks the property up in the *same* `ComponentMeta::properties` slice
//! [`crate::validate`] checks a value against, so a component's `props: […]`
//! entry is the only place its attribute surface is declared.

use rowan::TextRange;

use crate::ast::Element;
use crate::binding::{is_binding_expr, parse_binding, PropSource};
use crate::node::ViewNode;
use crate::registry::{ComponentMeta, PropKind};

/// [`Props::build_children_sized`]'s per-child result: the built node, plus
/// its own literal `Height` and `Width` attribute value (each `None` when
/// absent or not a number) — named so the field itself stays legible
/// (clippy's `type_complexity` lint, same reason `registry::BuildFn` is a
/// named alias rather than spelled out twice).
pub type SizedChild = (Box<dyn ViewNode>, Option<f32>, Option<f32>);

/// A build-time failure: a `component!` `build` closure could not turn an
/// element into a live node — a malformed binding, a value
/// [`crate::validate`] would also reject, or (defensively; should not happen
/// once [`crate::compile::compile`] only calls `build` after a clean
/// validation pass) an attribute that does not parse. Carries the byte range
/// to underline, exactly like [`crate::syntax::Diagnostic`], because it *is*
/// turned into one — see [`crate::compile::build_error_to_diagnostic`].
#[derive(Debug, Clone)]
pub struct BuildError {
    pub message: String,
    pub range: Option<TextRange>,
}

impl BuildError {
    pub fn new(message: impl Into<String>, range: Option<TextRange>) -> Self {
        Self { message: message.into(), range }
    }
}

/// Threaded through every `build` call. Deliberately thin today (phase 2c
/// builds a tree once per file change and does not yet need shared build-time
/// state); the type exists as the extension point later phases
/// (`x:Name` uniqueness, the `OUT_DIR` `FocusId` constants of §2's option B)
/// hang additions off, without changing every `build` closure's signature
/// again.
#[derive(Default)]
pub struct BuildCx {
    /// The folder of the view file, which relative image paths (`BackgroundImage`, `Image`, `Icon`)
    /// are resolved against; `None` for a view compiled from text alone.
    pub base_dir: Option<std::path::PathBuf>,
    /// Some element of the view has a `TabIndex`: every element then takes part in the Tab order by
    /// its index (0 when it has none).
    pub tab_order: bool,
    /// The named components built so far (DATA-2, `crate::scope`).
    pub(crate) components: Vec<crate::scope::Entry>,
    /// Instances of the previous tree a hot reload keeps: `(name, class, instance)`.
    pub(crate) reuse: crate::scope::Reusable,
    /// How many named components of each class were built (the generated names of unnamed ones).
    pub(crate) unnamed: Vec<(&'static str, usize)>,
    /// The floating parts of the view (`<Popover>`), painted above the whole view after it,
    /// wherever they are declared (`crate::node::TopLayerRoot`).
    pub(crate) top_layer: Vec<std::rc::Rc<std::cell::RefCell<dyn crate::node::TopLayer>>>,
}

impl std::fmt::Debug for BuildCx {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BuildCx").field("base_dir", &self.base_dir).field("tab_order", &self.tab_order).field("components", &self.components.len()).finish()
    }
}

impl BuildCx {
    pub fn new() -> Self {
        Self::default()
    }
}

/// The typed view of one XML element's attributes, handed to a component's
/// `build` closure (see `crate::registry::components` and the `component!`
/// macro's new `build:` clause).
pub struct Props<'a> {
    element: &'a Element,
    meta: &'static ComponentMeta,
}

impl<'a> Props<'a> {
    pub fn new(element: &'a Element, meta: &'static ComponentMeta) -> Self {
        Self { element, meta }
    }

    /// The element this view reads — for a `build` closure that needs
    /// something [`Props`] does not wrap directly (recursing into children:
    /// see [`Props::build_children`]/[`Props::build_single_child`], which
    /// cover the two children models every registered component uses).
    pub fn element(&self) -> &Element {
        self.element
    }

    /// The registry entry of the element (what a shared `build` — an application class's,
    /// EVT-7b — reads its name from).
    pub fn meta(&self) -> &'static ComponentMeta {
        self.meta
    }

    /// The attribute of property `name` as written: under its canonical name, else under one of its
    /// older aliases (`Max` for `Maximum`), whichever name the `build` closure asks for.
    fn raw(&self, name: &str) -> Option<(String, Option<TextRange>)> {
        let attr = self.element.attribute(name).or_else(|| {
            let meta = self.meta.property(name)?;
            std::iter::once(meta.name).chain(meta.aliases.iter().copied()).filter(|n| *n != name).find_map(|n| self.element.attribute(n))
        })?;
        Some((attr.value().unwrap_or_default(), attr.value_range()))
    }

    /// Whether the element writes property `name` (under any of its names).
    pub fn has(&self, name: &str) -> bool {
        self.raw(name).is_some()
    }

    fn err(&self, name: &str, range: Option<TextRange>, message: impl Into<String>) -> BuildError {
        BuildError::new(format!("attribute `{name}`: {}", message.into()), range.or_else(|| self.element.name_range()))
    }

    /// A `String`-typed property (`PropKind::String`): `Text`, `Placeholder`…
    pub fn str(&self, name: &str, default: &str) -> Result<PropSource<String>, BuildError> {
        match self.raw(name) {
            None => Ok(PropSource::Literal(default.to_string())),
            Some((raw, range)) if is_binding_expr(&raw) => {
                self.bound(name, &raw, range, default.to_string())
            }
            // An icon carries the element's `IconColor`/`IconSize`/`IconScaling` with it, and an
            // image file is made absolute against the view's folder (`crate::icon`).
            Some((raw, _)) if self.meta.property(name).is_some_and(|p| p.is_icon()) => {
                Ok(PropSource::Literal(crate::icon::with_options(&raw, |n| self.raw(n).map(|(v, _)| v))))
            }
            Some((raw, _)) => Ok(PropSource::Literal(raw)),
        }
    }

    /// A `Bool`-typed property (`PropKind::Bool`): `On`, `Dense`, `Loading`…
    pub fn bool(&self, name: &str, default: bool) -> Result<PropSource<bool>, BuildError> {
        match self.raw(name) {
            None => Ok(PropSource::Literal(default)),
            Some((raw, range)) if is_binding_expr(&raw) => self.bound(name, &raw, range, default),
            Some((raw, range)) => match raw.as_str() {
                "true" => Ok(PropSource::Literal(true)),
                "false" => Ok(PropSource::Literal(false)),
                _ => Err(self.err(name, range, format!("expected `true` or `false`, found `{raw}`"))),
            },
        }
    }

    /// An `F32`-typed property (`PropKind::F32`): `Gap`, `Padding`…
    pub fn f32(&self, name: &str, default: f32) -> Result<PropSource<f32>, BuildError> {
        match self.raw(name) {
            None => Ok(PropSource::Literal(default)),
            Some((raw, range)) if is_binding_expr(&raw) => self.bound(name, &raw, range, default),
            Some((raw, range)) => raw
                .parse::<f32>()
                .map(PropSource::Literal)
                .map_err(|_| self.err(name, range, format!("expected a number, found `{raw}`"))),
        }
    }

    /// An `Enum`-typed property (`PropKind::Enum`): the raw variant name
    /// (`"Primary"`, `"TopDown"`…), checked against the same variant list
    /// [`crate::validate`] checks it against when [`ComponentMeta::property`]
    /// has an entry for `name`. Returned as a plain `String` rather than a
    /// generic `T: FromStr` — the concrete `kubuno_ui`/`kubuno_controls` enum
    /// types have no `FromStr` impl (nothing in that crate needed one before
    /// this), and adding one there would be exactly the "touch `kubuno-ui`
    /// for XML views" this crate avoids (see the crate root doc). A `build`
    /// closure matches the string onto the real enum itself — one small
    /// `match`, not a trait bound this crate would have to invent.
    pub fn enum_(&self, name: &str, default: &str) -> Result<PropSource<String>, BuildError> {
        match self.raw(name) {
            None => Ok(PropSource::Literal(default.to_string())),
            Some((raw, range)) if is_binding_expr(&raw) => self.bound(name, &raw, range, default.to_string()),
            Some((raw, range)) => {
                if let Some(PropKind::Enum(variants)) = self.meta.property(name).map(|p| p.kind) {
                    if !variants.contains(&raw.as_str()) {
                        return Err(self.err(
                            name,
                            range,
                            format!("`{raw}` is not valid here; expected one of: {}", variants.join(", ")),
                        ));
                    }
                }
                Ok(PropSource::Literal(raw))
            }
        }
    }

    fn bound<T>(&self, name: &str, raw: &str, range: Option<TextRange>, fallback: T) -> Result<PropSource<T>, BuildError> {
        match parse_binding(raw) {
            Some(spec) => Ok(PropSource::Bound { spec, fallback }),
            None => Err(self.err(name, range, "malformed binding expression")),
        }
    }

    /// An event attribute's handler name (`OnClick="save"`) — a plain string,
    /// never a binding: §2's handler table is looked up by exactly this name.
    /// `None` when the element carries no such attribute, or an empty one.
    ///
    /// `name` may be the event's canonical attribute or one of its older aliases
    /// (`vskubuno/docs/EVENTS.md` §3, EVT-3): either way the element is read under the
    /// canonical name first, then under every alias, so `OnToggled="x"` on a `<Switch>`
    /// still binds its `OnCheckedChanged`, whichever of the two a `build` closure asks for.
    pub fn event(&self, name: &str) -> Option<String> {
        let read = |attr: &str| self.element.attribute(attr).and_then(|a| a.value()).filter(|s| !s.is_empty());
        match self.meta.event(name).or_else(|| crate::registry::view_event(name)) {
            Some(meta) => read(meta.name).or_else(|| meta.aliases.iter().find_map(|alias| read(alias))),
            None => read(name),
        }
    }

    /// `x:Name`, hashed into the [`kubuno_ui::FocusId`] it *is* (§1: "free …
    /// not a synthesized field name"). `None` for an element with no
    /// `x:Name` — it simply never takes part in [`kubuno_ui::FocusRing`].
    pub fn focus_id(&self) -> Option<kubuno_ui::FocusId> {
        self.element.attribute("x:Name").and_then(|a| a.value()).map(|s| crate::compile::scoped_focus(kubuno_ui::FocusId::of(&s)))
    }

    /// Builds every direct child element (`ChildrenModel::List`, e.g.
    /// `<Stack>`) into its own [`ViewNode`], in document order — one call
    /// each `build` closure of a list container makes instead of walking
    /// [`Element::children`] and the registry itself.
    pub fn build_children(&self, cx: &mut BuildCx) -> Result<Vec<Box<dyn ViewNode>>, BuildError> {
        self.element.children().map(|child| crate::compile::build_node(&child, cx, self.meta.layout)).collect()
    }

    /// [`Self::build_children`], each paired with its own literal `Height`/
    /// `Width` attribute value, when it has one that parses as a number —
    /// what `<Stack>` reads to let one child pin its own main-axis extent
    /// instead of trusting `ViewNode::measure`'s guess (`node::StackChild`'s
    /// own doc has the worked example, `<Splitter>`). Read directly off the
    /// child element rather than through a `{Binding …}`-aware `PropSource`:
    /// these two are a build-time layout override, the same "resolved once,
    /// never through a binding" posture `containers::literal_f32` already
    /// takes for Dock/Anchor's own `X`/`Y`/`Width`/`Height`.
    pub fn build_children_sized(&self, cx: &mut BuildCx) -> Result<Vec<SizedChild>, BuildError> {
        self.element
            .children()
            .map(|child| {
                let node = crate::compile::build_node(&child, cx, self.meta.layout)?;
                let auto = crate::common::auto_sized(&child);
                let height = if auto { None } else { child.attribute("Height") }.and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok());
                let width = if auto { None } else { child.attribute("Width") }.and_then(|a| a.value()).and_then(|v| v.trim().parse::<f32>().ok());
                Ok((node, height, width))
            })
            .collect()
    }

    /// Builds the single child element (`ChildrenModel::SingleWidget`, e.g.
    /// `<Card>`'s body), if there is one. [`crate::validate`] already rejects
    /// a second child for such a component, so `build` never needs to.
    pub fn build_single_child(&self, cx: &mut BuildCx) -> Result<Option<Box<dyn ViewNode>>, BuildError> {
        match self.element.children().next() {
            Some(child) => Ok(Some(crate::compile::build_node(&child, cx, self.meta.layout)?)),
            None => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{AstNode, Document};
    use crate::binding::BindingMode;
    use crate::registry;
    use crate::syntax::parse;

    fn element(src: &str) -> Element {
        let p = parse(src);
        let doc = Document::cast(p.syntax()).unwrap();
        doc.root_element().unwrap()
    }

    fn button_meta() -> &'static ComponentMeta {
        registry::lookup("Button").unwrap()
    }

    #[test]
    fn missing_attribute_is_the_declared_default() {
        let el = element(r#"<Button/>"#);
        let props = Props::new(&el, button_meta());
        let text = props.str("Text", "fallback").unwrap();
        assert!(matches!(text, PropSource::Literal(ref s) if s == "fallback"));
    }

    #[test]
    fn literal_bool_is_read_typed() {
        let el = element(r#"<Switch On="true"/>"#);
        let meta = registry::lookup("Switch").unwrap();
        let props = Props::new(&el, meta);
        let on = props.bool("On", false).unwrap();
        assert!(matches!(on, PropSource::Literal(true)));
    }

    #[test]
    fn bad_bool_is_a_diagnostic() {
        let el = element(r#"<Switch On="yes"/>"#);
        let meta = registry::lookup("Switch").unwrap();
        let props = Props::new(&el, meta);
        let err = props.bool("On", false).unwrap_err();
        assert!(err.message.contains("true"), "{}", err.message);
        assert!(err.range.is_some());
    }

    #[test]
    fn bad_number_is_a_diagnostic_with_a_range() {
        let el = element(r#"<Stack Gap="wide"/>"#);
        let meta = registry::lookup("Stack").unwrap();
        let props = Props::new(&el, meta);
        let err = props.f32("Gap", 8.0).unwrap_err();
        assert!(err.message.contains("expected a number"), "{}", err.message);
    }

    #[test]
    fn enum_value_is_checked_against_the_registered_variants() {
        let el = element(r#"<Button Variant="Sekondary"/>"#);
        let props = Props::new(&el, button_meta());
        let err = props.enum_("Variant", "Primary").unwrap_err();
        assert!(err.message.contains("Sekondary"), "{}", err.message);
        assert!(err.message.contains("Secondary"), "{}", err.message);
    }

    #[test]
    fn binding_expression_is_parsed_into_a_bound_source() {
        let el = element(r#"<Switch On="{Binding Notifications, Mode=TwoWay}"/>"#);
        let meta = registry::lookup("Switch").unwrap();
        let props = Props::new(&el, meta);
        let on = props.bool("On", false).unwrap();
        match on {
            PropSource::Bound { spec, fallback } => {
                assert_eq!(spec.path, "Notifications");
                assert_eq!(spec.mode, BindingMode::TwoWay);
                assert!(!fallback);
            }
            PropSource::Literal(_) => panic!("expected a binding"),
        }
    }

    #[test]
    fn malformed_binding_is_a_diagnostic() {
        let el = element(r#"<Switch On="{Notbinding}"/>"#);
        let meta = registry::lookup("Switch").unwrap();
        let props = Props::new(&el, meta);
        let err = props.bool("On", false).unwrap_err();
        assert!(err.message.contains("malformed binding"), "{}", err.message);
    }

    #[test]
    fn event_and_focus_id_are_read_from_reserved_attributes() {
        let el = element(r#"<Button x:Name="go" OnClick="save_clicked"/>"#);
        let props = Props::new(&el, button_meta());
        assert_eq!(props.event("OnClick").as_deref(), Some("save_clicked"));
        assert_eq!(props.event("OnMissing"), None);
        assert_eq!(props.focus_id(), Some(kubuno_ui::FocusId::of("go")));
    }

    #[test]
    fn an_event_is_read_under_its_canonical_name_or_an_alias() {
        let meta = registry::lookup("Switch").unwrap();
        let old = element(r#"<Switch OnToggled="old_style"/>"#);
        let props = Props::new(&old, meta);
        assert_eq!(props.event("OnToggled").as_deref(), Some("old_style"));
        assert_eq!(props.event("OnCheckedChanged").as_deref(), Some("old_style"));
        let new = element(r#"<Switch OnCheckedChanged="new_style"/>"#);
        let props = Props::new(&new, meta);
        assert_eq!(props.event("OnToggled").as_deref(), Some("new_style"));
    }
}
