//! # `kubuno-desktop-views-macros` — procedural macros of the `kubuno-desktop-views` event system
//!
//! Work packages EVT-1, EVT-4, EVT-7a and EVT-7b of `vskubuno/docs/EVENTS.md`: `#[derive(EventArgs)]`,
//! `#[event_handlers]`, `#[derive(Component)]` (the control hierarchy and, since EVT-7b, the
//! design-time metadata and registration of an application's controls), `#[derive(UserControl)]`
//! and `#[derive(PropertyValue)]`.
//! Use them through their re-exports, `kubuno_desktop_views::event_handlers`,
//! `kubuno_desktop_views::events::EventArgs` and `kubuno_desktop_views::component::Component` (a trait and its
//! derive share one name, like `serde::Serialize`): the generated code
//! names `::kubuno_desktop_views::…`, so this crate is useless on its own.

// Linking a proc-macro crate as a DLL makes MSVC's linker announce the import library it writes
// (« Creating library kubuno_desktop_views_macros-….dll.lib… »), which rustc forwards as a warning on every
// build (same as `kubuno_desktop_ui`). It is information, not a problem.
#![allow(linker_messages)]

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{parse_macro_input, Data, DeriveInput, Fields, GenericParam, Ident, LitStr, Path};

mod component;
mod handlers;
mod paths;
mod view;

/// Implements `kubuno_desktop_views::events::EventArgs` (and `ArgsChain`) for a struct,
/// plus `Handled` / `Cancelable` when asked to.
///
/// Every option goes in one or more `#[args(…)]` attributes:
///
/// | Option | Effect |
/// |---|---|
/// | `handled` / `handled = "field"` | implements `Handled` over a `bool` field (default name `handled`); `Event::raise` then stops at the first handler that sets it |
/// | `cancel` / `cancel = "field"` | implements `Cancelable` over a `bool` field (default name `cancel`) |
/// | `extends = Path` (or `"Path"`) | the ancestor used for the type chain (default: the root `EventArgs`); `Path` must itself implement `ArgsChain` |
/// | `legacy = path::to::fn` | `fn(&Self) -> Value`, the value the legacy `handlers!` table receives (default `Value::Bool(true)`) |
///
/// The type chain (`EventArgs::type_chain`, used by tooling to decide which
/// handlers are compatible with an event) is the struct's own name followed by
/// its ancestor's chain, computed at compile time:
///
/// ```
/// use kubuno_desktop_views::events::{ArgsChain, Cancelable, EventArgs, MouseEventArgs};
///
/// #[derive(EventArgs, Debug, Default)]
/// #[args(extends = MouseEventArgs, cancel)]
/// struct RulerDragArgs {
///     offset: f32,
///     cancel: bool,
/// }
///
/// assert_eq!(RulerDragArgs::CHAIN, ["RulerDragArgs", "MouseEventArgs", "EventArgs"]);
/// let mut e = RulerDragArgs::default();
/// e.set_cancel(true);
/// assert!(e.cancel());
/// assert!(e.as_cancelable().is_some() && e.as_handled().is_none());
/// ```
///
/// Only `'static` structs qualify (the trait extends `Any`): a lifetime
/// parameter is a compile error, and every type parameter gets a `'static`
/// bound.
///
/// ```compile_fail
/// use kubuno_desktop_views::events::EventArgs;
/// #[derive(EventArgs)]
/// struct Borrowing<'a> { text: &'a str }
/// ```
///
/// `handled` / `cancel` name a field that must exist:
///
/// ```compile_fail
/// use kubuno_desktop_views::events::EventArgs;
/// #[derive(EventArgs)]
/// #[args(handled)]
/// struct NoFlag { value: u32 }
/// ```
///
/// ```compile_fail
/// use kubuno_desktop_views::events::EventArgs;
/// #[derive(EventArgs)]
/// #[args(routing = "bubble")] // unknown option
/// struct Unknown;
/// ```
#[proc_macro_derive(EventArgs, attributes(args))]
pub fn derive_event_args(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match expand(input) {
        Ok(tokens) => paths::retarget(tokens).into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// The parsed `#[args(…)]` options.
#[derive(Default)]
struct Options {
    handled: Option<Ident>,
    cancel: Option<Ident>,
    extends: Option<Path>,
    legacy: Option<Path>,
}

fn parse_options(input: &DeriveInput) -> syn::Result<Options> {
    let mut opts = Options::default();
    for attr in input.attrs.iter().filter(|a| a.path().is_ident("args")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("handled") || meta.path.is_ident("cancel") {
                let is_handled = meta.path.is_ident("handled");
                let field = if meta.input.peek(syn::Token![=]) {
                    let lit: LitStr = meta.value()?.parse()?;
                    Ident::new(&lit.value(), lit.span())
                } else {
                    Ident::new(if is_handled { "handled" } else { "cancel" }, Span::call_site())
                };
                let slot = if is_handled { &mut opts.handled } else { &mut opts.cancel };
                if slot.replace(field).is_some() {
                    return Err(meta.error("duplicate option"));
                }
                Ok(())
            } else if meta.path.is_ident("extends") || meta.path.is_ident("legacy") {
                let is_extends = meta.path.is_ident("extends");
                let value = meta.value()?;
                let path: Path = if value.peek(LitStr) {
                    let lit: LitStr = value.parse()?;
                    lit.parse()?
                } else {
                    value.parse()?
                };
                let slot = if is_extends { &mut opts.extends } else { &mut opts.legacy };
                if slot.replace(path).is_some() {
                    return Err(meta.error("duplicate option"));
                }
                Ok(())
            } else {
                Err(meta.error("unknown `args` option (expected `handled`, `cancel`, `extends` or `legacy`)"))
            }
        })?;
    }
    Ok(opts)
}

/// Fails unless `field` is one of the struct's named fields.
fn check_field(input: &DeriveInput, field: &Ident, what: &str) -> syn::Result<()> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(&input.ident, "`#[derive(EventArgs)]` only supports structs"));
    };
    let found = match &data.fields {
        Fields::Named(named) => named.named.iter().any(|f| f.ident.as_ref() == Some(field)),
        _ => false,
    };
    if found {
        Ok(())
    } else {
        Err(syn::Error::new(
            field.span(),
            format!("`#[args({what})]` needs a `bool` field named `{field}`"),
        ))
    }
}

fn expand(mut input: DeriveInput) -> syn::Result<TokenStream2> {
    if !matches!(input.data, Data::Struct(_)) {
        return Err(syn::Error::new_spanned(&input.ident, "`#[derive(EventArgs)]` only supports structs"));
    }
    let opts = parse_options(&input)?;
    if let Some(f) = &opts.handled {
        check_field(&input, f, "handled")?;
    }
    if let Some(f) = &opts.cancel {
        check_field(&input, f, "cancel")?;
    }

    // `EventArgs: Any` — no borrowed args, and every type parameter `'static`.
    let mut static_bounds = Vec::new();
    for param in &input.generics.params {
        match param {
            GenericParam::Lifetime(lt) => {
                return Err(syn::Error::new_spanned(
                    lt,
                    "event args must be `'static` (`EventArgs: Any`); own the data instead of borrowing it",
                ));
            }
            GenericParam::Type(t) => {
                let ident = &t.ident;
                static_bounds.push(quote!(#ident: 'static));
            }
            GenericParam::Const(_) => {}
        }
    }
    {
        let where_clause = input.generics.make_where_clause();
        for b in static_bounds {
            where_clause.predicates.push(syn::parse2(b)?);
        }
    }

    let name = &input.ident;
    let name_str = LitStr::new(&name.to_string(), name.span());
    let writable = opts.handled.is_some() || opts.cancel.is_some();
    let (impl_g, ty_g, where_g) = input.generics.split_for_impl();
    let ev = quote!(::kubuno_desktop_views::events);

    let parent = match &opts.extends {
        Some(p) => quote!(#p),
        None => quote!(#ev::EmptyEventArgs),
    };

    let legacy = opts.legacy.as_ref().map(|path| {
        quote! {
            fn legacy_value(&self) -> ::kubuno_desktop_views::binding::Value {
                #path(self)
            }
        }
    });

    let (handled_hooks, handled_impl) = match &opts.handled {
        Some(field) => (
            quote! {
                fn as_handled(&self) -> ::core::option::Option<&dyn #ev::Handled> { ::core::option::Option::Some(self) }
                fn as_handled_mut(&mut self) -> ::core::option::Option<&mut dyn #ev::Handled> { ::core::option::Option::Some(self) }
            },
            quote! {
                impl #impl_g #ev::Handled for #name #ty_g #where_g {
                    fn handled(&self) -> bool { self.#field }
                    fn set_handled(&mut self, v: bool) { self.#field = v; }
                }
            },
        ),
        None => (TokenStream2::new(), TokenStream2::new()),
    };

    let (cancel_hooks, cancel_impl) = match &opts.cancel {
        Some(field) => (
            quote! {
                fn as_cancelable(&self) -> ::core::option::Option<&dyn #ev::Cancelable> { ::core::option::Option::Some(self) }
                fn as_cancelable_mut(&mut self) -> ::core::option::Option<&mut dyn #ev::Cancelable> { ::core::option::Option::Some(self) }
            },
            quote! {
                impl #impl_g #ev::Cancelable for #name #ty_g #where_g {
                    fn cancel(&self) -> bool { self.#field }
                    fn set_cancel(&mut self, v: bool) { self.#field = v; }
                }
            },
        ),
        None => (TokenStream2::new(), TokenStream2::new()),
    };

    // Args without `handled`/`cancel` can be copied into an async handler (EVT-6).
    let read_only_impl = if writable {
        TokenStream2::new()
    } else {
        quote!(impl #impl_g #ev::ReadOnlyArgs for #name #ty_g #where_g {})
    };

    Ok(quote! {
        #read_only_impl

        impl #impl_g #ev::ArgsChain for #name #ty_g #where_g {
            const NAME: &'static str = #name_str;
            const RUST_TYPE: &'static str = #name_str;
            const WRITABLE: bool = #writable;
            const CHAIN: &'static [&'static str] = {
                const PARENT: &[&str] = <#parent as #ev::ArgsChain>::CHAIN;
                const LEN: usize = PARENT.len() + 1;
                const ARR: [&str; LEN] = {
                    let mut out = [""; LEN];
                    out[0] = #name_str;
                    let mut i = 0;
                    while i < PARENT.len() {
                        out[i + 1] = PARENT[i];
                        i += 1;
                    }
                    out
                };
                &ARR
            };
        }

        impl #impl_g #ev::EventArgs for #name #ty_g #where_g {
            fn as_any(&self) -> &dyn ::core::any::Any { self }
            fn as_any_mut(&mut self) -> &mut dyn ::core::any::Any { self }
            fn type_chain(&self) -> &'static [&'static str] {
                <Self as #ev::ArgsChain>::CHAIN
            }
            #legacy
            #handled_hooks
            #cancel_hooks
        }

        #handled_impl
        #cancel_impl
    })
}

/// Typed event handlers: marks the `impl` block of a view model whose methods handle the
/// events its view names (`OnClick="say_hello_click"`), and implements
/// `kubuno_desktop_views::events::EventSink` for the type (`vskubuno/docs/EVENTS.md` §5.4, EVT-4).
/// Paint with `Runtime::frame_typed` so the view's events reach them.
///
/// ```
/// use kubuno_desktop_views::prelude::*;
///
/// #[derive(Default)]
/// struct MainViewModel { status: String }
///
/// impl ViewModel for MainViewModel {
///     fn get(&self, _path: &str) -> Option<Value> { None }
///     fn set(&mut self, _path: &str, _value: Value) {}
/// }
///
/// #[kubuno_desktop_views::event_handlers]
/// impl MainViewModel {
///     fn say_hello_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {
///         self.status = format!("{} at {}, {}", sender.text(), e.x, e.y);
///     }
///     fn name_key_down(&mut self, e: &mut KeyEventArgs) { e.handled = true; }
///     fn window_load(&mut self) {}
///     #[handler(name = "save")]
///     fn save_clicked(&mut self, _e: &dyn EventArgs) {}
///     #[handler(skip)]
///     fn helper(&self) -> usize { self.status.len() }
/// }
///
/// assert_eq!(MainViewModel::HANDLERS.len(), 4);
/// assert_eq!(MainViewModel::HANDLERS[0].args, Some("MouseEventArgs"));
/// ```
///
/// Each method with a `&mut self` or `&self` receiver is a handler (associated functions
/// without `self` are left alone), named like the method unless `#[handler(name = "…")]`;
/// `#[handler(skip)]` excludes one. After the receiver it takes nothing, the event args
/// (`e: &A` or `e: &mut A` for an args type `A`, `&dyn EventArgs` for any), the sender
/// (`&Sender<Control>` or `&ElementRef`), or the sender then the args. The parameters are
/// exempt from the unused-variable lint (the event fixes the signature, like WinForms'), the
/// method bodies are not.
///
/// An `async fn` is an **async handler** (EVT-6): it runs as a task on the UI thread and, since a
/// borrow of the view model cannot live across `.await`, takes no `self` but an optional
/// `ui: UiHandle<Self>` (then `ui.update(|vm| …)`) and an optional copy of the args, by value:
///
/// ```
/// # use kubuno_desktop_views::prelude::*;
/// # use std::time::Duration;
/// # #[derive(Default)] struct Vm { status: String }
/// # impl ViewModel for Vm {
/// #     fn get(&self, _: &str) -> Option<Value> { None }
/// #     fn set(&mut self, _: &str, _: Value) {}
/// # }
/// #[kubuno_desktop_views::event_handlers]
/// impl Vm {
///     async fn save_click(ui: UiHandle<Self>, e: MouseEventArgs) {
///         ui.update(|vm| vm.status = "Saving…".into());
///         delay(Duration::from_millis(200)).await;
///         ui.update(|vm| vm.status = "Saved.".into());
///     }
/// }
/// assert!(Vm::HANDLERS[0].asynchronous);
/// ```
///
/// Anything else is a compile error that says what is expected:
///
/// ```compile_fail
/// # use kubuno_desktop_views::prelude::*;
/// # struct Vm;
/// #[kubuno_desktop_views::event_handlers]
/// impl Vm {
///     async fn save_click(&mut self, e: &mut MouseEventArgs) {} // async: `ui: UiHandle<Self>`, not `&mut self`
/// }
/// ```
///
/// ```compile_fail
/// # use kubuno_desktop_views::prelude::*;
/// # struct Vm;
/// # impl ViewModel for Vm {
/// #     fn get(&self, _: &str) -> Option<Value> { None }
/// #     fn set(&mut self, _: &str, _: Value) {}
/// # }
/// #[kubuno_desktop_views::event_handlers]
/// impl Vm {
///     // `cancel` cannot be set after an `.await`: FormClosing needs a synchronous handler.
///     async fn closing(ui: UiHandle<Self>, e: FormClosingEventArgs) {}
/// }
/// ```
///
/// ```compile_fail
/// # use kubuno_desktop_views::prelude::*;
/// # struct Vm;
/// #[kubuno_desktop_views::event_handlers]
/// impl Vm {
///     fn save_click(&mut self, e: MouseEventArgs) {} // args by reference
/// }
/// ```
///
/// ```compile_fail
/// # use kubuno_desktop_views::prelude::*;
/// # struct Vm;
/// #[kubuno_desktop_views::event_handlers]
/// impl Vm {
///     fn save_click(&mut self, e: &MouseEventArgs, sender: &Sender<Button>) {} // sender first
/// }
/// ```
///
/// ```compile_fail
/// # use kubuno_desktop_views::prelude::*;
/// # struct Vm;
/// #[kubuno_desktop_views::event_handlers]
/// impl Vm {
///     fn save_click(&mut self, e: &u32) {} // not an args type
/// }
/// ```
///
/// ```compile_fail
/// # use kubuno_desktop_views::prelude::*;
/// # struct Vm;
/// #[kubuno_desktop_views::event_handlers]
/// impl Vm {
///     fn save_click(&mut self, sender: &Sender<String>) {} // not a control type
/// }
/// ```
///
/// ```compile_fail
/// # use kubuno_desktop_views::prelude::*;
/// # struct Vm;
/// #[kubuno_desktop_views::event_handlers]
/// impl Vm {
///     fn save_click(self, e: &MouseEventArgs) {} // `&mut self` or `&self`
/// }
/// ```
///
/// ```compile_fail
/// # use kubuno_desktop_views::prelude::*;
/// # struct Vm;
/// #[kubuno_desktop_views::event_handlers]
/// impl Vm {
///     fn save_click(&mut self, e: &MouseEventArgs) -> bool { true } // returns nothing
/// }
/// ```
///
/// ```compile_fail
/// # use kubuno_desktop_views::prelude::*;
/// # struct Vm;
/// #[kubuno_desktop_views::event_handlers]
/// impl Vm {
///     fn save(&mut self) {}
///     #[handler(name = "save")]
///     fn save_again(&mut self) {} // two handlers named `save`
/// }
/// ```
#[proc_macro_attribute]
pub fn event_handlers(attr: TokenStream, item: TokenStream) -> TokenStream {
    paths::retarget(handlers::expand(attr.into(), item.into())).into()
}

/// Registers a value converter (`Converter=Name` in a `{Binding …}`) before `main`: put it on the
/// `impl kubuno_desktop_views::binding::ValueConverter for T` block of a type that implements `Default`.
/// The name is the type's, or the one given: `#[value_converter("Initials")]`.
///
/// ```
/// use kubuno_desktop_views::binding::{ValueConverter, Value};
///
/// #[derive(Default)]
/// struct Initials;
///
/// #[kubuno_desktop_views::value_converter]
/// impl ValueConverter for Initials {
///     fn convert(&self, value: Option<Value>, _parameter: Option<&str>) -> Option<Value> {
///         match value? {
///             Value::Str(s) => Some(Value::Str(s.split_whitespace().filter_map(|w| w.chars().next()).collect())),
///             _ => None,
///         }
///     }
/// }
/// ```
#[proc_macro_attribute]
pub fn value_converter(attr: TokenStream, item: TokenStream) -> TokenStream {
    paths::retarget(expand_value_converter(attr.into(), item.into()).unwrap_or_else(syn::Error::into_compile_error)).into()
}

fn expand_value_converter(attr: TokenStream2, item: TokenStream2) -> syn::Result<TokenStream2> {
    let imp: syn::ItemImpl = syn::parse2(item)?;
    let Some((_, trait_path, _)) = &imp.trait_ else {
        return Err(syn::Error::new_spanned(&imp.self_ty, "`#[value_converter]` goes on an `impl ValueConverter for Type` block"));
    };
    if trait_path.segments.last().is_none_or(|s| s.ident != "ValueConverter") {
        return Err(syn::Error::new_spanned(trait_path, "`#[value_converter]` goes on an `impl ValueConverter for Type` block"));
    }
    let ty = &imp.self_ty;
    let name = if attr.is_empty() {
        match &**ty {
            syn::Type::Path(p) => p.path.segments.last().map(|s| s.ident.to_string()).unwrap_or_default(),
            other => return Err(syn::Error::new_spanned(other, "name the converter: `#[value_converter(\"Name\")]`")),
        }
    } else {
        syn::parse2::<LitStr>(attr)?.value()
    };
    if name.is_empty() {
        return Err(syn::Error::new_spanned(ty, "a converter needs a name"));
    }
    Ok(quote! {
        #imp
        ::kubuno_desktop_views::register_value_converter!(#name, #ty);
    })
}

/// A class of the control hierarchy (`kubuno_desktop_views::component`, EVT-7a of
/// `vskubuno/docs/EVENTS.md`): implements the plumbing of the levels the class belongs to — the
/// base-state accessors, the delegation to the base object, the upcasts, `base()` / `base_mut()`,
/// the type chain (`Lineage`, `ClassInfo`) and the `Sender` typing (`ElementType`) — and an empty
/// `impl` of every level trait (`Component`, `Control`, `ButtonBase`…) except those listed in
/// `overrides(…)`, which the class writes itself with the methods it overrides.
///
/// | Option (in `#[kubuno(…)]`) | Effect |
/// |---|---|
/// | `extends = Base` | the base: a built-in class (`Button`, `Label`, `Panel`… — the base field is that class) or a level (`Control`, `ButtonBase`, `Component`… — the base field is its core, `ButtonBaseCore`) |
/// | `overrides(Control, ButtonBase, …)` | the level traits the class implements itself (`impl Control for RoundButton { fn on_paint(…) }`) |
/// | `levels(ButtonBase, …)` | only when `Base` is a class of your own: the levels it belongs to (the macro cannot see it) |
///
/// The base field is the one named `base`, or the one marked `#[kubuno(base)]`.
///
/// ```
/// use kubuno_desktop_views::prelude::*;
///
/// #[derive(Component)]
/// #[kubuno(extends = Button, overrides(Control))]
/// struct RoundButton { base: Button }
///
/// impl Control for RoundButton {
///     fn on_click(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
///         self.base_mut().on_click(e); // base.OnClick(e)
///     }
/// }
///
/// /// A non-visual component, like a WinForms component in the tray.
/// #[derive(Component, Default)]
/// #[kubuno(extends = Component)]
/// struct Clock { base: ComponentCore, interval_ms: u32 }
///
/// /// A control written from the `Control` level up.
/// #[derive(Component, Default)]
/// #[kubuno(extends = Control)]
/// struct Gauge { #[kubuno(base)] core: ControlCore, level: f32 }
///
/// /// A class of your own as the base: name its levels.
/// #[derive(Component)]
/// #[kubuno(extends = RoundButton, levels(ButtonBase))]
/// struct BigRoundButton { base: RoundButton }
///
/// assert_eq!(<BigRoundButton as Lineage>::CHAIN, ["BigRoundButton", "RoundButton", "Button", "ButtonBase", "Control", "Component"]);
/// assert_eq!(<Clock as Lineage>::CHAIN, ["Clock", "Component"]);
/// let g = Gauge::default();
/// assert_eq!(g.base().styles, ControlStyles::CONTROL_DEFAULT);
/// ```
///
/// `extends` is required:
///
/// ```compile_fail
/// use kubuno_desktop_views::prelude::*;
/// #[derive(Component)]
/// struct NoBase { base: Button }
/// ```
///
/// The base field must be what `extends` names:
///
/// ```compile_fail
/// use kubuno_desktop_views::prelude::*;
/// #[derive(Component)]
/// #[kubuno(extends = Button)]
/// struct Wrong { base: Label }
/// ```
///
/// A level the class implements itself must be listed in `overrides(…)` (otherwise the macro's
/// empty `impl Control` conflicts with yours):
///
/// ```compile_fail
/// use kubuno_desktop_views::prelude::*;
/// #[derive(Component)]
/// #[kubuno(extends = Button)]
/// struct Round { base: Button }
/// impl Control for Round {}
/// ```
///
/// … and a level listed there must then be implemented:
///
/// ```compile_fail
/// use kubuno_desktop_views::prelude::*;
/// #[derive(Component)]
/// #[kubuno(extends = Button, overrides(Control))]
/// struct Round { base: Button }
/// fn needs_control(_: &dyn Control) {}
/// fn f(r: &Round) { needs_control(r) }
/// ```
///
/// `overrides` names levels of the class's own chain:
///
/// ```compile_fail
/// use kubuno_desktop_views::prelude::*;
/// #[derive(Component)]
/// #[kubuno(extends = Button, overrides(ListControl))]
/// struct Round { base: Button }
/// ```
///
/// A class of your own as the base needs its levels:
///
/// ```compile_fail
/// use kubuno_desktop_views::prelude::*;
/// #[derive(Component)]
/// #[kubuno(extends = Button)]
/// struct Round { base: Button }
/// #[derive(Component)]
/// #[kubuno(extends = Round)]
/// struct Rounder { base: Round }
/// ```
///
/// A class is a concrete struct with named fields, and not generic:
///
/// ```compile_fail
/// use kubuno_desktop_views::prelude::*;
/// #[derive(Component)]
/// #[kubuno(extends = Button)]
/// struct Tuple(Button);
/// ```
///
/// ```compile_fail
/// use kubuno_desktop_views::prelude::*;
/// #[derive(Component)]
/// #[kubuno(extends = Button)]
/// struct Generic<T> { base: Button, value: T }
/// ```
#[proc_macro_derive(
    Component,
    attributes(kubuno, property, event, category, description, default_value, browsable, default_event, default_property, toolbox, localizable, designer_serialization_visibility, editor, type_converter)
)]
pub fn derive_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match component::expand(input, kubuno_desktop_views_meta::Derive::Component) {
        Ok(tokens) => paths::retarget(tokens).into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// A user control (EVT-7b of `vskubuno/docs/EVENTS.md`): a composite control designed as a
/// `.kbcontrol` view of its own, whose root is `<UserControl x:Class="RatingBar">`, usable as an element
/// of the program's other views (`<RatingBar Max="10" OnValueCommitted="…"/>`).
///
/// It is a class of the hierarchy extending the `UserControl` level (base field
/// `base: UserControlCore`), and **its own view model**: the `{Binding …}`s of its view read and
/// write its `#[property]` fields (by their XML names), and the `On*` handlers of its view are the
/// methods of its `#[kubuno_desktop_views::event_handlers]` impl. Its `#[event]` fields are the events it
/// raises to the view using it (`self.raise_value_committed(args)` re-raises an inner event).
///
/// ```
/// use kubuno_desktop_views::prelude::*;
///
/// /// A row of stars.
/// #[derive(UserControl, Default)]
/// #[user_control(view = "../tests/fixtures/rating_bar.kbcontrol", default_event = "ValueCommitted")]
/// pub struct RatingBar {
///     base: UserControlCore,
///     /// The number of stars.
///     #[property(bindable)]
///     #[category("Behavior")]
///     #[default_value(5)]
///     pub max: u32,
///     #[property(bindable)]
///     pub value: u32,
///     /// Occurs when the user picks a rating.
///     #[event]
///     pub value_committed: Event<EmptyEventArgs>,
/// }
///
/// #[kubuno_desktop_views::event_handlers]
/// impl RatingBar {
///     fn star_click(&mut self) {
///         self.value += 1;
///         self.raise_value_committed(EmptyEventArgs);
///     }
/// }
///
/// let mut r = RatingBar::default();
/// r.max = 10;
/// assert_eq!(r.get("Max"), Some(Value::F32(10.0)));
/// r.set("Value", Value::F32(3.0));
/// assert_eq!(r.value, 3);
/// ```
///
/// A user control names its view:
///
/// ```compile_fail
/// use kubuno_desktop_views::prelude::*;
/// #[derive(UserControl, Default)]
/// struct NoView { base: UserControlCore }
/// ```
#[proc_macro_derive(
    UserControl,
    attributes(kubuno, property, event, category, description, default_value, browsable, default_event, default_property, toolbox, localizable, designer_serialization_visibility, editor, type_converter, user_control)
)]
pub fn derive_user_control(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match component::expand(input, kubuno_desktop_views_meta::Derive::UserControl) {
        Ok(tokens) => paths::retarget(tokens).into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// A fieldless enum usable as the type of a control's `#[property]` (EVT-7b): its variants are
/// the attribute's XML values, offered in the Properties window's drop-down and by completion.
///
/// ```
/// use kubuno_desktop_views::prelude::*;
///
/// #[derive(PropertyValue, Default, Clone, Copy, PartialEq, Debug)]
/// enum Shape { #[default] Pill, Square }
///
/// assert_eq!(Shape::from_value(&Value::Str("Square".into())), Some(Shape::Square));
/// assert_eq!(Shape::Pill.to_value(), Value::Str("Pill".into()));
/// ```
#[proc_macro_derive(PropertyValue)]
pub fn derive_property_value(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    match component::expand_property_value(input) {
        Ok(tokens) => paths::retarget(tokens).into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// A Windows Forms-like form class over a `.kbview` view — use it as `#[kubuno_desktop::view("main_view.kbview")]`
/// (the generated code names `::kubuno_desktop::…`; see the `kubuno-desktop` crate's documentation of `View`).
///
/// The path is relative to the Rust file holding the attribute (like `include_str!`); the view is
/// read at compile time and embedded, and editing it rebuilds the crate. The attribute adds one field
/// per `x:Name`d control, typed with its handle (`status: kubuno_desktop::TextField`), a hidden
/// `kubuno_desktop::Form`, `initialize_component()`, and implements `kubuno_desktop::View` (each handler the view
/// names — `OnClick="hello_click"` — runs the struct's method of that name), `ViewModel`, `Deref` to
/// `kubuno_desktop::Form`, `Default` (unless derived), `show()` and `show_dialog(owner)`. `#[bind]` on a
/// field makes it the source of `{Binding FieldName}` (PascalCase; `#[bind("Path")]` names it), and
/// one `#[data_context]` field answers the other binding paths.
#[proc_macro_attribute]
pub fn view(attr: TokenStream, item: TokenStream) -> TokenStream {
    view::expand(attr.into(), item.into()).into()
}
