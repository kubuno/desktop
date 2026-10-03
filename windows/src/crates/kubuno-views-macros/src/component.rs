//! `#[derive(Component)]` (EVT-7a of `vskubuno/docs/EVENTS.md`): the plumbing of a class of the
//! control hierarchy (`kubuno_views::component`), and since EVT-7b its design-time metadata
//! (properties, events, Toolbox) and its registration, so that an application's own controls
//! are usable as XML elements; `#[derive(UserControl)]` adds a composite's view and view model.

use kubuno_views_meta::{self as meta, ComponentDecl, DeclKind, Derive};
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, Ident, LitStr, Type};

/// The field named `field` of the struct.
fn field_type(input: &DeriveInput, field: &str) -> Option<Type> {
    let Data::Struct(data) = &input.data else { return None };
    let Fields::Named(fields) = &data.fields else { return None };
    fields.named.iter().find(|f| f.ident.as_ref().is_some_and(|i| i == field)).map(|f| f.ty.clone())
}

fn field_span(input: &DeriveInput, field: &str) -> proc_macro2::Span {
    let Data::Struct(data) = &input.data else { return input.ident.span() };
    let Fields::Named(fields) = &data.fields else { return input.ident.span() };
    fields.named.iter().find(|f| f.ident.as_ref().is_some_and(|i| i == field)).map(|f| f.span()).unwrap_or_else(|| input.ident.span())
}

/// The crate being compiled (`CARGO_CRATE_NAME`, set by cargo and by the Visual Studio design
/// build). The built-in classes of `kubuno_views` itself are not registered: the registry
/// already has them.
fn crate_name() -> String {
    std::env::var("CARGO_CRATE_NAME").unwrap_or_default()
}

/// The absolute path of the file being expanded (`None` outside a real expansion: unit tests).
fn declaring_file() -> Option<std::path::PathBuf> {
    if !proc_macro::is_available() {
        return None;
    }
    let file = proc_macro::Span::call_site().local_file()?;
    Some(if file.is_relative() { std::env::current_dir().ok()?.join(file) } else { file })
}

/// The absolute folder of the view `view_path` (relative to `declaring_file`, like `include_str!`), normalized
/// (`..` folded); `None` without a declaring file (unit tests) or when `view_path` has no folder to name.
fn view_folder(declaring_file: Option<&std::path::Path>, view_path: &str) -> Option<String> {
    let base = declaring_file?.parent()?;
    let file = base.join(view_path.replace('\\', "/"));
    let mut out = std::path::PathBuf::new();
    for part in file.parent()?.components() {
        match part {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out.is_absolute().then(|| out.to_string_lossy().into_owned())
}

pub(crate) fn expand(mut input: DeriveInput, derive: Derive) -> syn::Result<TokenStream2> {
    if let Some(param) = input.generics.params.first() {
        return Err(syn::Error::new_spanned(param, "a component class cannot be generic: it is a concrete `'static` type the registry and the designer name"));
    }
    // A user control extends the `UserControl` level (its base field is a `UserControlCore`).
    if derive == Derive::UserControl && meta::parse_kubuno_options(&input.attrs).is_ok_and(|o| o.extends.is_none()) {
        input.attrs.push(syn::parse_quote!(#[kubuno(extends = UserControl)]));
    }
    // An inherited user control (`#[kubuno(extends = AddressEditor)]`, another user control of the application): it is
    // a UserControl too, whatever the macro cannot see of its base.
    let inherits_user_control = derive == Derive::UserControl
        && meta::parse_kubuno_options(&input.attrs).is_ok_and(|o| {
            o.extends.as_ref().and_then(|p| p.segments.last()).is_some_and(|s| meta::builtin_chain(&s.ident.to_string()).is_empty()) && o.levels.is_empty()
        });
    if inherits_user_control {
        input.attrs.push(syn::parse_quote!(#[kubuno(levels(UserControl))]));
    }
    let opts = meta::parse_kubuno_options(&input.attrs)?;
    let Some(extends) = opts.extends.clone() else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "missing `#[kubuno(extends = …)]`: name the base class (`Button`, `Label`…) or level (`Control`, `ButtonBase`, `Component`…)",
        ));
    };
    let decl = meta::parse_decl(&input, derive)?;
    let (base, base_ty) = meta::base_field(&input)?;
    let Some(extends_ident) = extends.segments.last().map(|s| s.ident.clone()) else {
        return Err(syn::Error::new_spanned(&extends, "expected a type name"));
    };
    let extends_name = extends_ident.to_string();
    if derive == Derive::UserControl && !meta::builtin_chain(&extends_name).contains(&"UserControl") && opts.levels.iter().all(|l| l != "UserControl") {
        return Err(syn::Error::new_spanned(&extends, "a user control extends `UserControl` (its base field is a `UserControlCore`)"));
    }

    // Which levels the class belongs to, and whether its base is a level's core or a class.
    let base_is_level = meta::level(&extends_name).is_some();
    let mut levels: Vec<&'static str> = if base_is_level {
        if !opts.levels.is_empty() {
            return Err(syn::Error::new_spanned(&opts.levels[0], "`levels(…)` is only for a base class the macro does not know: a level base already names its levels"));
        }
        meta::level_chain(&extends_name)
    } else if let Some(lvl) = meta::class_level(&extends_name) {
        if !opts.levels.is_empty() {
            return Err(syn::Error::new_spanned(&opts.levels[0], format!("`{extends_name}` is a built-in class: its levels are known, remove `levels(…)`")));
        }
        meta::level_chain(lvl)
    } else if opts.levels.is_empty() {
        return Err(syn::Error::new_spanned(
            &extends,
            format!(
                "`{extends_name}` is not a built-in class or level: name the levels it belongs to, e.g. `#[kubuno(extends = {extends_name}, levels(ButtonBase))]` (the levels are {})",
                meta::all_level_names()
            ),
        ));
    } else {
        let mut all = Vec::new();
        for l in &opts.levels {
            for n in meta::level_chain(&l.to_string()) {
                if !all.contains(&n) {
                    all.push(n);
                }
            }
        }
        all
    };
    if !levels.contains(&"Component") {
        levels.push("Component");
    }
    for o in &opts.overrides {
        let name = o.to_string();
        if !levels.contains(&name.as_str()) {
            return Err(syn::Error::new(o.span(), format!("`{name}` is not a level of `{}` (its levels: {})", input.ident, levels.join(", "))));
        }
    }

    let name = &input.ident;
    let name_str = LitStr::new(&name.to_string(), name.span());
    let k = quote!(::kubuno_views::component);
    let span = extends.span();

    // The base-field type check: a class base is that class; a level base is the level's core.
    let expected = if base_is_level {
        let core = format_ident!("{}Core", extends_name);
        quote!(#k::#core)
    } else {
        quote!(#extends)
    };

    // The base is an object of every level of the chain: a class, or a level's core (the
    // abstract base, whose methods are the root behaviour).
    let some_base = |dyn_trait: TokenStream2, mutable: bool| -> TokenStream2 {
        if mutable {
            quote!(::core::option::Option::Some(&mut self.#base as &mut dyn #dyn_trait))
        } else {
            quote!(::core::option::Option::Some(&self.#base as &dyn #dyn_trait))
        }
    };

    let mut upcasts = TokenStream2::new();
    let mut links = TokenStream2::new();
    let mut level_impls = TokenStream2::new();
    for lvl in &levels {
        let Some((_, _, stem)) = meta::level(lvl) else { continue };
        let trait_ident = Ident::new(lvl, span);
        if !opts.overrides.iter().any(|o| o == lvl) {
            level_impls.extend(quote_spanned!(span=> impl #k::#trait_ident for #name {}));
        }
        if *lvl == "Component" {
            continue;
        }
        let as_ref = format_ident!("as_{}", stem);
        let as_mut = format_ident!("as_{}_mut", stem);
        upcasts.extend(quote! {
            fn #as_ref(&self) -> ::core::option::Option<&dyn #k::#trait_ident> { ::core::option::Option::Some(self) }
            fn #as_mut(&mut self) -> ::core::option::Option<&mut dyn #k::#trait_ident> { ::core::option::Option::Some(self) }
        });
        let has = format_ident!("Has{}Core", lvl);
        let core = format_ident!("{}Core", lvl);
        let get = format_ident!("{}_core", stem);
        let get_mut = format_ident!("{}_core_mut", stem);
        let link = format_ident!("{}Link", lvl);
        let base_get = format_ident!("base_{}", stem);
        let base_get_mut = format_ident!("base_{}_mut", stem);
        let some = some_base(quote!(#k::#trait_ident), false);
        let some_mut = some_base(quote!(#k::#trait_ident), true);
        links.extend(quote! {
            impl #k::#has for #name {
                fn #get(&self) -> &#k::#core { #k::#has::#get(&self.#base) }
                fn #get_mut(&mut self) -> &mut #k::#core { #k::#has::#get_mut(&mut self.#base) }
            }
            impl #k::#link for #name {
                fn #base_get(&self) -> ::core::option::Option<&dyn #k::#trait_ident> { #some }
                fn #base_get_mut(&mut self) -> ::core::option::Option<&mut dyn #k::#trait_ident> { #some_mut }
            }
        });
    }

    let base_component = some_base(quote!(#k::Component), false);
    let base_component_mut = some_base(quote!(#k::Component), true);

    // EVT-7b: the declared properties (set from XML, read by bindings), the declared events'
    // raise methods, the user control's view model, and the registration.
    let properties = property_access(&input, &decl)?;
    let events = event_raisers(&input, &decl)?;
    let (view_model, view_model_link) = if derive == Derive::UserControl {
        user_control_view_model(&input, &decl, (!meta::builtin_chain(&extends_name).contains(&"UserControl")).then_some(&base))?
    } else {
        (TokenStream2::new(), TokenStream2::new())
    };
    let registration = if crate_name() == "kubuno_views" { TokenStream2::new() } else { registration(&input, &decl, &extends_name)? };

    Ok(quote! {
        const _: () = {
            // The base field must be what `extends` names.
            #[allow(dead_code)]
            fn __kubuno_base_is_the_extended_type(c: &#name) -> &#expected {
                &c.#base
            }
        };

        impl #name {
            /// The embedded base (WinForms `base`): call its overridden behaviour with
            /// `self.base_mut().on_paint(e)`.
            #[allow(dead_code)]
            pub fn base(&self) -> &#base_ty {
                &self.#base
            }

            /// The embedded base, mutably.
            #[allow(dead_code)]
            pub fn base_mut(&mut self) -> &mut #base_ty {
                &mut self.#base
            }
        }

        impl #k::Lineage for #name {
            const CHAIN: &'static [&'static str] = {
                const PARENT: &[&str] = <#base_ty as #k::Lineage>::CHAIN;
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

        impl #k::ClassInfo for #name {
            const NAME: &'static str = #name_str;
            type Base = #base_ty;
        }

        impl ::kubuno_views::events::ElementType for #name {
            const ELEMENT: &'static str = #name_str;
            type Resolved = ::kubuno_views::events::ElementProps;
        }

        impl #k::HasComponentCore for #name {
            fn component_core(&self) -> &#k::ComponentCore { #k::HasComponentCore::component_core(&self.#base) }
            fn component_core_mut(&mut self) -> &mut #k::ComponentCore { #k::HasComponentCore::component_core_mut(&mut self.#base) }
        }

        impl #k::ComponentLink for #name {
            fn as_any(&self) -> &dyn ::core::any::Any { self }
            fn as_any_mut(&mut self) -> &mut dyn ::core::any::Any { self }
            fn as_component(&self) -> &dyn #k::Component { self }
            fn as_component_mut(&mut self) -> &mut dyn #k::Component { self }
            fn class_name(&self) -> &'static str { #name_str }
            fn class_chain(&self) -> &'static [&'static str] { <Self as #k::Lineage>::CHAIN }
            fn base_component(&self) -> ::core::option::Option<&dyn #k::Component> { #base_component }
            fn base_component_mut(&mut self) -> ::core::option::Option<&mut dyn #k::Component> { #base_component_mut }
            #upcasts
            #properties
            #view_model_link
        }

        #links
        #level_impls
        #events
        #view_model
        #registration
    })
}

/// `self.update_bar();` for a property declared `#[property(on_change = "update_bar")]` (the body of a Windows
/// Forms property setter), nothing otherwise.
fn on_change_call(p: &meta::PropertyDecl, input: &DeriveInput) -> TokenStream2 {
    match &p.on_change {
        Some(method) => {
            let method = Ident::new(method, field_span(input, &p.field));
            quote!(self.#method();)
        }
        None => TokenStream2::new(),
    }
}

/// `kubuno_set_property` / `kubuno_get_property` over the declared properties (the XML attributes
/// of the element, and the user control's bindings); unknown names go to the base.
fn property_access(input: &DeriveInput, decl: &ComponentDecl) -> syn::Result<TokenStream2> {
    if decl.properties.is_empty() {
        return Ok(TokenStream2::new());
    }
    let value = quote!(::kubuno_views::binding::Value);
    let pv = quote!(::kubuno_views::component::PropertyValue);
    let mut set_arms = TokenStream2::new();
    let mut get_arms = TokenStream2::new();
    for p in &decl.properties {
        let Some(ty) = field_type(input, &p.field) else { continue };
        let field = Ident::new(&p.field, field_span(input, &p.field));
        let attr = &p.name;
        let span = ty.span();
        let changed = on_change_call(p, input);
        set_arms.extend(quote_spanned! {span=>
            #attr => match <#ty as #pv>::from_value(value) {
                ::core::option::Option::Some(v) => { self.#field = v; #changed true }
                ::core::option::Option::None => false,
            },
        });
        get_arms.extend(quote_spanned! {span=>
            #attr => ::core::option::Option::Some(<#ty as #pv>::to_value(&self.#field)),
        });
    }
    Ok(quote! {
        fn kubuno_set_property(&mut self, name: &str, value: &#value) -> bool {
            match name {
                #set_arms
                _ => match ::kubuno_views::component::ComponentLink::base_component_mut(self) {
                    ::core::option::Option::Some(base) => base.kubuno_set_property(name, value),
                    ::core::option::Option::None => false,
                },
            }
        }
        fn kubuno_get_property(&self, name: &str) -> ::core::option::Option<#value> {
            match name {
                #get_arms
                _ => ::kubuno_views::component::ComponentLink::base_component(self).and_then(|base| base.kubuno_get_property(name)),
            }
        }
    })
}

/// One `raise_<field>(args)` per `#[event]` field: raises the field's `Event` to its Rust
/// subscribers, then to the element's `.kbview` handler (WinForms' `OnValueCommitted(e)`).
fn event_raisers(input: &DeriveInput, decl: &ComponentDecl) -> syn::Result<TokenStream2> {
    if decl.events.is_empty() {
        return Ok(TokenStream2::new());
    }
    let name = &input.ident;
    let mut methods = TokenStream2::new();
    for e in &decl.events {
        let Some(ty) = field_type(input, &e.field) else { continue };
        let Some((_, args_ty)) = meta::event_args(&ty) else { continue };
        let field = Ident::new(&e.field, field_span(input, &e.field));
        let method = format_ident!("raise_{}", e.field.trim_start_matches("r#"));
        let attr = &e.name;
        let display = attr.strip_prefix("On").unwrap_or(attr);
        let doc = format!("Raises `{display}`: the `{}` field's Rust subscribers, then the `{attr}` handler of the element in its view (queued until the element next paints, in the same frame).", e.field);
        methods.extend(quote! {
            #[doc = #doc]
            #[allow(dead_code)]
            pub fn #method(&mut self, args: #args_ty) {
                let event = ::core::clone::Clone::clone(&self.#field);
                ::kubuno_views::component::raise_declared_event(self, #attr, &event, args);
            }
        });
    }
    Ok(quote! {
        impl #name {
            #methods
        }
    })
}

/// A user control is its own view model: its inner `{Binding …}`s read and write its declared
/// properties, and its inner events run its `#[event_handlers]` methods.
fn user_control_view_model(input: &DeriveInput, decl: &ComponentDecl, base_user_control: Option<&Ident>) -> syn::Result<(TokenStream2, TokenStream2)> {
    let name = &input.ident;
    let value = quote!(::kubuno_views::binding::Value);
    let pv = quote!(::kubuno_views::component::PropertyValue);
    let mut get_arms = TokenStream2::new();
    let mut set_arms = TokenStream2::new();
    for p in &decl.properties {
        let Some(ty) = field_type(input, &p.field) else { continue };
        let field = Ident::new(&p.field, field_span(input, &p.field));
        let attr = &p.name;
        get_arms.extend(quote! { #attr => ::core::option::Option::Some(<#ty as #pv>::to_value(&self.#field)), });
        let changed = on_change_call(p, input);
        set_arms.extend(quote! { #attr => { if let ::core::option::Option::Some(v) = <#ty as #pv>::from_value(&value) { self.#field = v; #changed } } });
    }
    // An inherited user control: what its own properties and handlers do not answer goes to the base user control
    // (the bindings and handlers of the base view it inherits).
    let vm = quote!(::kubuno_views::binding::ViewModel);
    let (get_rest, set_rest, dispatch_rest) = match base_user_control {
        Some(base) => (
            quote!(#vm::get(&self.#base, path)),
            quote!(#vm::set(&mut self.#base, path, value)),
            quote!(#vm::dispatch_event(&mut self.#base, handler, sender, args)),
        ),
        None => (quote!(::core::option::Option::None), quote!({ let _ = value; }), quote!(false)),
    };
    let view_model = quote! {
        impl ::kubuno_views::binding::ViewModel for #name {
            fn get(&self, path: &str) -> ::core::option::Option<#value> {
                match path {
                    #get_arms
                    _ => #get_rest,
                }
            }
            fn set(&mut self, path: &str, value: #value) {
                match path {
                    #set_arms
                    _ => #set_rest,
                }
            }
            fn dispatch_event(&mut self, handler: &str, sender: &::kubuno_views::events::ElementRef<'_>, args: &mut dyn ::kubuno_views::events::EventArgs) -> bool {
                #[allow(unused_imports)]
                use ::kubuno_views::registry::__private::{SinkDispatch as _, SinkNone as _};
                (&::kubuno_views::registry::__private::Probe::<Self>::new()).kubuno_dispatch(self, handler, sender, args) || #dispatch_rest
            }
        }
    };
    let link = quote! {
        fn kubuno_view_model(&mut self) -> ::core::option::Option<&mut dyn ::kubuno_views::binding::ViewModel> { ::core::option::Option::Some(self) }
    };
    Ok((view_model, link))
}

fn opt_str(value: &Option<String>) -> TokenStream2 {
    match value {
        Some(s) => quote!(::core::option::Option::Some(#s)),
        None => quote!(::core::option::Option::None),
    }
}

/// The class's registration (`kubuno_views::registry::ClassRegistration`): its metadata, a
/// factory, and a static constructor that registers it when the program starts (before `main`),
/// so a view can name it as soon as it is compiled into the program — no call to write.
fn registration(input: &DeriveInput, decl: &ComponentDecl, extends: &str) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let r = quote!(::kubuno_views::registry);
    let pv = quote!(::kubuno_views::component::PropertyValue);

    let mut props = Vec::new();
    for p in &decl.properties {
        let Some(ty) = field_type(input, &p.field) else { continue };
        let (attr, doc, default) = (&p.name, &p.description, &p.default_value);
        let span = ty.span();
        let mut e = quote_spanned!(span=> #r::PropertyMeta::new(#attr, <#ty as #pv>::KIND, #default, #doc));
        if let Some(c) = &p.category {
            e = quote!(#e.category(#c));
        }
        if !p.browsable {
            e = quote!(#e.hidden());
        }
        if p.bindable {
            e = quote!(#e.bindable());
        }
        if p.localizable {
            e = quote!(#e.localizable());
        }
        if let Some(v) = &p.serialization {
            e = quote!(#e.serialization(#v));
        }
        if let Some(v) = &p.editor {
            e = quote!(#e.editor(#v));
        } else {
            // A list (`Rows`) or any Rust value (`Shared<T>`): its type names its editor.
            e = quote_spanned!(span=> #e.value_editor(<#ty as #pv>::EDITOR));
        }
        if let Some(v) = &p.type_converter {
            e = quote!(#e.type_converter(#v));
        }
        props.push(e);
    }

    let mut events = Vec::new();
    for ev in &decl.events {
        let Some(ty) = field_type(input, &ev.field) else { continue };
        let Some((_, args_ty)) = meta::event_args(&ty) else { continue };
        let (attr, doc) = (&ev.name, &ev.description);
        let category = Ident::new(&ev.category.replace(' ', ""), name.span());
        let span = args_ty.span();
        let mut e = quote_spanned!(span=> #r::EventMeta::new(#attr, #doc).category(#r::EventCategory::#category).args::<#args_ty>());
        if !ev.browsable {
            e = quote!(#e.hidden());
        }
        events.push(e);
    }
    let (n_props, n_events) = (props.len(), events.len());

    let kind = match decl.kind {
        DeclKind::Control => quote!(#r::ClassKind::Control),
        DeclKind::UserControl => quote!(#r::ClassKind::UserControl),
        DeclKind::Component => quote!(#r::ClassKind::Component),
    };
    let name_str = name.to_string();
    let crate_name = crate_name();
    let doc = &decl.description;
    let default_event = opt_str(&decl.default_event);
    let default_property = opt_str(&decl.default_property);
    let toolbox_category = opt_str(&decl.toolbox_category);
    // A toolbox bitmap (WinForms' `[ToolboxBitmap]`): an image next to the source file, registered by
    // its absolute path (what the Toolbox loads) and tracked like `include_bytes!` (a compile error
    // when it is missing, a rebuild when it changes).
    let (toolbox_icon, bitmap_track) = match decl.toolbox_icon.as_deref().filter(|i| meta::is_image_path(i)) {
        Some(icon) => {
            let Some(path) = declaring_file().and_then(|f| meta::resolve_toolbox_bitmap(icon, &f)).filter(|p| p.is_file()).or_else(|| {
                let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR")?);
                meta::resolve_toolbox_bitmap(icon, &manifest.join("src").join("lib.rs")).filter(|p| p.is_file())
            }) else {
                return Err(syn::Error::new_spanned(name, format!("the toolbox bitmap `{icon}` was not found: the path is relative to the Rust file that declares `{name}`")));
            };
            let abs = path.to_string_lossy().into_owned();
            (quote!(::core::option::Option::Some(#abs)), quote!(const _: &[u8] = ::core::include_bytes!(#abs);))
        }
        None => (opt_str(&decl.toolbox_icon), TokenStream2::new()),
    };
    let browsable = decl.browsable;
    let (view, view_path) = match &decl.view {
        Some(path) => {
            let lit = LitStr::new(path, name.span());
            // An inherited user control's view (`x:Inherits="address_editor.kbcontrol"`): the merged view is embedded, its
            // own file and its bases tracked.
            let file = declaring_file().map(|f| f.parent().map(|d| d.join(path)).unwrap_or_default());
            let merged = file.as_ref().and_then(|f| std::fs::read_to_string(f).ok().map(|text| (f.clone(), text))).and_then(|(f, text)| {
                kubuno_views_meta::inherit::inherits(&text)?;
                Some(kubuno_views_meta::inherit::resolve(&text, Some(&f), &|p| std::fs::read_to_string(p)))
            });
            match merged {
                Some(Ok(Some((view, files)))) => {
                    let tracked: Vec<String> = files.iter().map(|f| f.to_string_lossy().into_owned()).collect();
                    (
                        quote!(::core::option::Option::Some({ const _: &str = ::core::include_str!(#lit); #(const _: &str = ::core::include_str!(#tracked);)* #view })),
                        quote!(::core::option::Option::Some(#lit)),
                    )
                }
                Some(Err(message)) => return Err(syn::Error::new_spanned(name, format!("the view `{path}` of `{name}`: {message}"))),
                _ => (quote!(::core::option::Option::Some(::core::include_str!(#lit))), quote!(::core::option::Option::Some(#lit))),
            }
        }
        None => (quote!(::core::option::Option::None), quote!(::core::option::Option::None)),
    };
    // The folder of the view, absolute: a user control nested by a view of another folder still resolves its own
    // relative paths (`d:ItemsSource="design/rows.json"`, images) next to its `.kbcontrol`.
    let view_dir = match decl.view.as_deref().and_then(|path| view_folder(declaring_file().as_deref(), path)) {
        Some(dir) => quote!(::core::option::Option::Some(#dir)),
        None => quote!(::core::option::Option::None),
    };

    Ok(quote! {
        #bitmap_track
        const _: () = {
            static __KUBUNO_PROPERTIES: [#r::PropertyMeta; #n_props] = [#(#props),*];
            static __KUBUNO_EVENTS: [#r::EventMeta; #n_events] = [#(#events),*];

            fn __kubuno_create() -> ::core::option::Option<::std::rc::Rc<::core::cell::RefCell<dyn ::kubuno_views::component::Component>>> {
                #[allow(unused_imports)]
                use #r::__private::{CreateDefault as _, CreateNone as _};
                (&#r::__private::Probe::<#name>::new()).kubuno_create()
            }

            static __KUBUNO_REGISTRATION: #r::ClassRegistration = #r::ClassRegistration {
                name: #name_str,
                crate_name: #crate_name,
                kind: #kind,
                doc: #doc,
                extends: #extends,
                chain: <#name as ::kubuno_views::component::Lineage>::CHAIN,
                create: __kubuno_create,
                properties: &__KUBUNO_PROPERTIES,
                events: &__KUBUNO_EVENTS,
                default_event: #default_event,
                default_property: #default_property,
                toolbox_category: #toolbox_category,
                toolbox_icon: #toolbox_icon,
                browsable: #browsable,
                view: #view,
                view_path: #view_path,
                view_dir: #view_dir,
                source_file: ::core::file!(),
            };

            impl #r::Registered for #name {
                fn registration() -> &'static #r::ClassRegistration {
                    &__KUBUNO_REGISTRATION
                }
            }

            // Runs before `main` (a static constructor, like C++'s): registers the class with the
            // view registry, so `<#name …/>` compiles in any view of the program.
            #[used]
            #[cfg_attr(windows, unsafe(link_section = ".CRT$XCU"))]
            #[cfg_attr(any(target_os = "linux", target_os = "android", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"), unsafe(link_section = ".init_array"))]
            #[cfg_attr(target_vendor = "apple", unsafe(link_section = "__DATA,__mod_init_func"))]
            static __KUBUNO_REGISTER: extern "C" fn() = {
                extern "C" fn register() {
                    #r::register_class(&__KUBUNO_REGISTRATION);
                }
                register
            };
        };
    })
}

/// `#[derive(PropertyValue)]` on a fieldless enum: its variants are the XML values.
pub(crate) fn expand_property_value(input: DeriveInput) -> syn::Result<TokenStream2> {
    let variants = meta::parse_property_enum(&input)?;
    if let Some(param) = input.generics.params.first() {
        return Err(syn::Error::new_spanned(param, "a property value enum cannot be generic"));
    }
    let name = &input.ident;
    let idents: Vec<Ident> = variants.iter().map(|v| Ident::new(v, name.span())).collect();
    let texts: Vec<&String> = variants.iter().collect();
    Ok(quote! {
        impl ::kubuno_views::component::PropertyValue for #name {
            const KIND: ::kubuno_views::registry::PropKind = ::kubuno_views::registry::PropKind::Enum(&[#(#texts),*]);
            fn from_value(value: &::kubuno_views::binding::Value) -> ::core::option::Option<Self> {
                match value {
                    ::kubuno_views::binding::Value::Str(s) => match s.trim() {
                        #(#texts => ::core::option::Option::Some(Self::#idents),)*
                        _ => ::core::option::Option::None,
                    },
                    _ => ::core::option::Option::None,
                }
            }
            fn to_value(&self) -> ::kubuno_views::binding::Value {
                ::kubuno_views::binding::Value::Str(::std::string::String::from(match self {
                    #(Self::#idents => #texts,)*
                }))
            }
        }
    })
}

/// Unit tests on the expansion's decisions (the compile-fail doctests of `lib.rs` cover the
/// error messages end to end).
#[cfg(test)]
mod tests {
    use super::*;

    fn expand_str(src: &str) -> Result<String, String> {
        let input: DeriveInput = syn::parse_str(src).map_err(|e| e.to_string())?;
        expand(input, Derive::Component).map(|t| t.to_string()).map_err(|e| e.to_string())
    }

    fn expand_uc(src: &str) -> Result<String, String> {
        let input: DeriveInput = syn::parse_str(src).map_err(|e| e.to_string())?;
        expand(input, Derive::UserControl).map(|t| t.to_string()).map_err(|e| e.to_string())
    }

    #[test]
    fn chains_follow_the_parents() {
        assert_eq!(meta::level_chain("View"), ["View", "ContainerControl", "ScrollableControl", "Control", "Component"]);
        assert_eq!(meta::level_chain("ButtonBase"), ["ButtonBase", "Control", "Component"]);
        assert_eq!(meta::level_chain("Component"), ["Component"]);
        for (class, lvl) in meta::CLASSES {
            // A class derives from a level, or from another built-in class (the ribbon family: `RibbonToggleButton`
            // from `RibbonButton`), whose chain then reaches a level.
            assert!(
                meta::level(lvl).is_some() || (meta::CLASSES.iter().any(|(c, _)| c == lvl) && meta::class_level(class).is_some()),
                "{class} derives from an unknown level or class {lvl}"
            );
        }
    }

    #[test]
    fn a_class_base_delegates_and_skips_the_overridden_level() {
        let out = expand_str("#[kubuno(extends = Button, overrides(Control))] struct RoundButton { base: Button, n: u32 }").unwrap();
        assert!(out.contains("impl :: kubuno_views :: component :: ButtonBase for RoundButton { }"));
        assert!(out.contains("impl :: kubuno_views :: component :: Component for RoundButton { }"));
        assert!(!out.contains("impl :: kubuno_views :: component :: Control for RoundButton"));
        assert!(out.contains("Some (& mut self . base as & mut dyn :: kubuno_views :: component :: Control)"));
        assert!(out.contains("fn as_button_base"));
        assert!(!out.contains("fn as_list_control"));
    }

    #[test]
    fn a_level_base_is_its_core() {
        let out = expand_str("#[kubuno(extends = ButtonBase)] struct MyButton { #[kubuno(base)] core: ButtonBaseCore }").unwrap();
        assert!(out.contains("Some (& self . core as & dyn :: kubuno_views :: component :: Control)"));
        assert!(out.contains(":: kubuno_views :: component :: ButtonBaseCore"));
        assert!(out.contains("& self . core"));
    }

    #[test]
    fn errors_name_the_fix() {
        let err = |src: &str| expand_str(src).unwrap_err();
        assert!(err("struct A { base: Button }").contains("missing `#[kubuno(extends"));
        assert!(err("#[kubuno(extends = Button)] struct A { b: Button }").contains("add a field `base"));
        assert!(err("#[kubuno(extends = Mystery)] struct A { base: Mystery }").contains("levels(ButtonBase)"));
        assert!(err("#[kubuno(extends = Button, overrides(ListControl))] struct A { base: Button }").contains("not a level of `A`"));
        assert!(err("#[kubuno(extends = Button, overrides(Widget))] struct A { base: Button }").contains("not a level of the hierarchy"));
        assert!(err("#[kubuno(extends = Button, levels(Control))] struct A { base: Button }").contains("built-in class"));
        assert!(err("#[kubuno(extends = Button, color = 1)] struct A { base: Button }").contains("unknown `kubuno` option"));
        assert!(err("#[kubuno(extends = Button)] struct A<T> { base: Button, t: T }").contains("cannot be generic"));
        assert!(err("#[kubuno(extends = Button)] enum A { X }").contains("only supports structs"));
        assert!(err("#[kubuno(extends = Button)] struct A(Button);").contains("named fields"));
        assert!(err("#[kubuno(extends = Button)] struct A { #[kubuno(base)] a: Button, #[kubuno(base)] b: Button }").contains("only one field"));
        assert!(expand_uc("#[kubuno(extends = Button)] #[user_control(view = \"a.kbcontrol\")] struct A { base: Button }").unwrap_err().contains("extends `UserControl`"));
        assert!(expand_uc("struct A { base: UserControlCore }").unwrap_err().contains("names its view"));
    }

    #[test]
    fn an_unknown_base_takes_its_levels() {
        let out = expand_str("#[kubuno(extends = RoundButton, levels(ButtonBase))] struct Fancy { base: RoundButton }").unwrap();
        assert!(out.contains("impl :: kubuno_views :: component :: ButtonBase for Fancy { }"));
        assert!(out.contains("impl :: kubuno_views :: component :: Control for Fancy { }"));
    }

    #[test]
    fn span_of_extends_is_used() {
        // A smoke check that expansion never panics on a path base.
        assert!(expand_str("#[kubuno(extends = kubuno_views::controls::Button)] struct A { base: kubuno_views::controls::Button }").is_ok());
    }

    /// EVT-7b: declared properties become settable by name, events get a raise method, and the
    /// class registers itself with its metadata.
    #[test]
    fn properties_events_and_the_registration_are_generated() {
        let out = expand_str(
            "#[kubuno(extends = Button, overrides(Control))] #[category(\"Kubuno\")] #[toolbox(icon = \"circle\")]
             struct RoundButton { base: Button,
                #[property] #[category(\"Appearance\")] #[default_value(18.0)] #[description(\"Radius.\")] corner_radius: f32,
                #[event] #[category(\"Mouse\")] long_press: Event<MouseEventArgs> }",
        )
        .unwrap();
        assert!(out.contains("fn kubuno_set_property"));
        assert!(out.contains("\"CornerRadius\" => match < f32 as :: kubuno_views :: component :: PropertyValue > :: from_value (value)"));
        assert!(out.contains("PropertyMeta :: new (\"CornerRadius\" , < f32 as :: kubuno_views :: component :: PropertyValue > :: KIND , \"18.0\" , \"Radius.\") . category (\"Appearance\")"));
        assert!(out.contains("EventMeta :: new (\"OnLongPress\" , \"\") . category (:: kubuno_views :: registry :: EventCategory :: Mouse) . args :: < MouseEventArgs > ()"));
        assert!(out.contains("pub fn raise_long_press (& mut self , args : MouseEventArgs)"));
        assert!(out.contains("toolbox_icon : :: core :: option :: Option :: Some (\"circle\")"));
        assert!(out.contains("register_class (& __KUBUNO_REGISTRATION)"));
        assert!(out.contains("link_section = \".CRT$XCU\""));
        assert!(!out.contains("impl :: kubuno_views :: binding :: ViewModel"));
    }

    #[test]
    fn a_user_control_is_its_own_view_model_and_embeds_its_view() {
        let out = expand_uc(
            "#[user_control(view = \"rating_bar.kbcontrol\")] struct RatingBar { base: UserControlCore, #[property(bindable)] max: u32 }",
        )
        .unwrap();
        assert!(out.contains("impl :: kubuno_views :: component :: UserControl for RatingBar { }"));
        assert!(out.contains("impl :: kubuno_views :: binding :: ViewModel for RatingBar"));
        assert!(out.contains("\"Max\" => :: core :: option :: Option :: Some (< u32 as :: kubuno_views :: component :: PropertyValue > :: to_value (& self . max))"));
        assert!(out.contains("include_str ! (\"rating_bar.kbcontrol\")"));
        assert!(out.contains("fn kubuno_view_model"));
        assert!(out.contains("ClassKind :: UserControl"));
        // No declaring file in a unit test: no folder registered.
        assert!(out.contains("view_dir : :: core :: option :: Option :: None"));
    }

    /// A user control's view folder is absolute and normalized, wherever the control sits in the crate.
    #[test]
    fn the_view_folder_is_next_to_the_declaring_file() {
        let root = if cfg!(windows) { "C:\\app\\src" } else { "/app/src" };
        let file = std::path::Path::new(root).join("admin").join("storage_block.rs");
        let expect = |parts: &[&str]| parts.iter().fold(std::path::PathBuf::from(root), |p, s| p.join(s)).to_string_lossy().into_owned();
        assert_eq!(view_folder(Some(&file), "storage_block.kbcontrol"), Some(expect(&["admin"])));
        assert_eq!(view_folder(Some(&file), "views/block.kbcontrol"), Some(expect(&["admin", "views"])));
        assert_eq!(view_folder(Some(&file), "../controls/block.kbcontrol"), Some(expect(&["controls"])));
        assert_eq!(view_folder(Some(&file), "./block.kbcontrol"), Some(expect(&["admin"])));
        assert_eq!(view_folder(None, "block.kbcontrol"), None);
        assert_eq!(view_folder(Some(std::path::Path::new("relative/x.rs")), "x.kbcontrol"), None);
    }

    /// "Substituer des membres…" adds `#[kubuno(overrides(UserControl))]` to a user control: it still extends
    /// `UserControl`.
    #[test]
    fn a_user_control_can_override_its_level() {
        let out = expand_uc("#[kubuno(overrides(UserControl))] #[user_control(view = \"a.kbcontrol\")] struct A { base: UserControlCore }").unwrap();
        assert!(!out.contains("impl :: kubuno_views :: component :: UserControl for A { }"));
        assert!(out.contains("impl :: kubuno_views :: component :: Control for A { }"));
    }

    #[test]
    fn property_value_enums() {
        let input: DeriveInput = syn::parse_str("enum Shape { Pill, Square }").unwrap();
        let out = expand_property_value(input).unwrap().to_string();
        assert!(out.contains("PropKind :: Enum (& [\"Pill\" , \"Square\"])"));
        let input: DeriveInput = syn::parse_str("enum Shape { Pill(u8) }").unwrap();
        assert!(expand_property_value(input).unwrap_err().to_string().contains("without fields"));
    }
}
