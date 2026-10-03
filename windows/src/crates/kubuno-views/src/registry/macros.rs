//! The `component!` declarative table, per `XML_VIEWS.md` §4: "`kubuno-ui`
//! itself already leans on `macro_rules!` to remove this kind of boilerplate
//! … the registry should follow the same idiom: one declarative table per
//! family". See `mod.rs`'s module doc for why the `smoke` block exists and
//! what it replaces from the note's own sketch.

/// Declares one component's metadata, plus a `#[test]` that exercises the
/// real `kubuno-ui` constructor and setters the metadata describes, plus (new
/// in phase 2c) the `build` function the interpreter calls to turn a parsed
/// `<Name>` element into a live [`crate::node::ViewNode`].
///
/// `layout:` is optional (`vskubuno/docs/DESIGNER.md` §4/§6 DSG-1: "a small,
/// additive registry field") and defaults to
/// [`crate::registry::LayoutKind::None`] when omitted — most components are
/// plain leaves or `SingleWidget` bodies with no layout engine of their own,
/// so only the handful of real containers (`Panel`, `Stack`, `Splitter`,
/// `Tabs`) need to name one, and every other one of this macro's ~50 other
/// call sites needed no mechanical update to gain the field.
///
/// ```ignore
/// component! {
///     mod_name: button,
///     name: "Button",
///     doc: "A push button.",
///     ctor: kubuno_ui::buttons::Button::new(""),
///     children: ChildrenModel::None,
///     props: [
///         PropertyMeta::new("text", PropKind::String, "", "The label."),
///     ],
///     events: [
///         EventMeta::new("OnClick", "Raised on activation."),
///     ],
///     smoke: |b| {
///         let b = b.variant(kubuno_ui::buttons::Variant::Secondary);
///         b
///     },
///     build: |props, cx| {
///         let text = props.str("Text", "")?;
///         Ok(Box::new(ButtonNode::new(text, /* … */)) as Box<dyn ViewNode>)
///     },
/// }
/// ```
macro_rules! component {
    (
        mod_name: $mod_name:ident,
        name: $name:literal,
        doc: $doc:literal,
        ctor: $ctor:expr,
        children: $children:expr,
        $(layout: $layout:expr,)?
        $(open_attributes: $open_attributes:expr,)?
        $(default_event: $default_event:literal,)?
        props: [ $( $prop:expr ),* $(,)? ],
        events: [ $( $event:expr ),* $(,)? ],
        smoke: |$binder:pat_param| $smoke:block,
        build: |$props_pat:pat_param, $cx_pat:pat_param| $build:block $(,)?
    ) => {
        // Metadata for the component named `$name` below.
        pub mod $mod_name {
            // `LayoutKind` is only actually referenced (through a call site's
            // own `layout: LayoutKind::…`, which resolves against ITS OWN
            // file-top import under normal macro hygiene, not this one) by
            // the handful of declarations that name a non-default layout;
            // every other one only uses this import transitively through the
            // `@layout` default arm's fully-qualified `$crate::registry::
            // LayoutKind::None`, so it is legitimately unused here for most
            // components.
            #[allow(unused_imports)]
            use $crate::registry::{ChildrenModel, ComponentMeta, EventMeta, LayoutKind, PropKind, PropertyMeta};
            #[allow(unused_imports)] // Not every `build` block needs every one of these.
            use $crate::node::{ButtonNode, CardNode, StackNode, SwitchNode, TextFieldNode, ViewNode};
            #[allow(unused_imports)]
            use $crate::props::{BuildCx, BuildError, Props};

            pub const PROPERTIES: &[PropertyMeta] = &[ $( $prop ),* ];
            pub const EVENTS: &[EventMeta] = &[ $( $event ),* ];

            /// Builds the real `kubuno_ui` widget tree for one `<$name>`
            /// element — see [`crate::compile::build_node`], the only
            /// caller. Reads every property through [`Props`]'s typed
            /// accessors, keyed by the same names `PROPERTIES` above
            /// declares (`crate::props`'s module doc: "one table", not a
            /// second hand-written setter list).
            pub fn build($props_pat: &Props<'_>, $cx_pat: &mut BuildCx) -> Result<Box<dyn ViewNode>, BuildError> {
                $build
            }

            pub const META: ComponentMeta = ComponentMeta {
                name: $name,
                doc: $doc,
                properties: PROPERTIES,
                events: EVENTS,
                children: $children,
                layout: $crate::registry::macros::component!(@layout $($layout)?),
                open_attributes: $crate::registry::macros::component!(@open_attributes $($open_attributes)?),
                default_event: $crate::registry::macros::component!(@default_event $($default_event)?),
                build,
            };

            // The drift guard `XML_VIEWS.md` §8's 2a row asks for: builds the
            // real component and calls every setter the metadata above
            // claims exists. A `kubuno-ui` rename/removal breaks this at
            // `cargo test`, not silently — see `mod.rs`'s module doc.
            #[cfg(test)]
            #[test]
            fn ctor_and_setters_match_kubuno_ui() {
                let $binder = $ctor;
                let _ = $smoke;
            }
        }
    };

    // Internal helper arms for the optional `layout:` field above (leading
    // `@layout` is not a legal start of the main arm's `mod_name: …`, so
    // there is no ambiguity between them): `(@layout)` is what an omitted
    // `layout:` expands the field to, `(@layout $layout:expr)` is the
    // explicit value. Not part of this macro's public call shape — only the
    // main arm's own expansion invokes these, recursively, through
    // `$crate::registry::macros::component!`.
    (@layout) => {
        $crate::registry::LayoutKind::None
    };
    (@layout $layout:expr) => {
        $layout
    };

    // Same idiom as `@layout`, for the optional `open_attributes:` field.
    (@open_attributes) => {
        false
    };
    (@open_attributes $open_attributes:expr) => {
        $open_attributes
    };

    // Same idiom, for the optional `default_event:` field (EVT-3): `None` falls back to
    // `ComponentMeta::default_event`'s rule (OnClick, else the first own event).
    (@default_event) => {
        None
    };
    (@default_event $default_event:literal) => {
        Some($default_event)
    };
}

pub(crate) use component;
