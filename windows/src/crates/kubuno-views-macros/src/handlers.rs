//! `#[event_handlers]` (EVT-4): see the attribute's documentation in `lib.rs`.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::spanned::Spanned;
use syn::{FnArg, GenericArgument, ImplItem, ImplItemFn, ItemImpl, LitStr, PathArguments, ReturnType, Type, TypeReference};

/// What `#[event_handlers]` generates for `item`: the `impl` itself (without the `#[handler]`
/// helper attributes) and the `EventSink` impl, or the `impl`, an empty `EventSink` impl and
/// the errors, so a mistake in one handler reports only that mistake.
pub fn expand(attr: TokenStream2, item: TokenStream2) -> TokenStream2 {
    let mut imp: ItemImpl = match syn::parse2(item.clone()) {
        Ok(imp) => imp,
        Err(_) => {
            // The input is re-emitted UNCHANGED, so the compiler and rust-analyzer keep analysing
            // the file as written; the only error added is the real one, at its real tokens.
            if looks_like_impl(&item) {
                // A syntax error inside the `impl` (a method body being typed). Every item of
                // the impl that parses is kept and expanded as usual (so the rest of the view
                // model, its other handlers and the `EventSink` stay visible to the compiler and
                // rust-analyzer); a method whose body does not parse keeps its signature with a
                // placeholder body. The syntax error itself is not repeated here: the compiler
                // parses the item before expanding it, and rust-analyzer parses the file, so both
                // already report it once, at its own token (adding syn's copy showed it twice).
                if let Some(mut recovered) = recover_impl(&item) {
                    let expanded = match expand_impl(attr, &mut recovered) {
                        Ok(tokens) => tokens,
                        Err(e) => {
                            strip_helper_attributes(&mut recovered);
                            let e = e.to_compile_error();
                            let sink = sink_impl(&recovered, &[], &[]);
                            quote!(#recovered #sink #e)
                        }
                    };
                    return expanded;
                }
                return item;
            }
            let span = item.clone().into_iter().next().map_or_else(Span::call_site, |t| t.span());
            let err = syn::Error::new(
                span,
                "`#[event_handlers]` goes on the `impl` block of the view model whose methods handle the view's events (`impl MainViewModel { … }`)",
            )
            .to_compile_error();
            return quote!(#item #err);
        }
    };
    match expand_impl(attr, &mut imp) {
        Ok(tokens) => tokens,
        Err(err) => {
            strip_helper_attributes(&mut imp);
            let err = err.to_compile_error();
            let sink = sink_impl(&imp, &[], &[]);
            quote!(#imp #sink #err)
        }
    }
}

/// Whether `item` is an `impl` block (attributes, `unsafe`, `default` aside) — one syn could not
/// parse because of a syntax error inside it, not something else the attribute was put on.
fn looks_like_impl(item: &TokenStream2) -> bool {
    use proc_macro2::TokenTree;
    let mut tokens = item.clone().into_iter().peekable();
    while let Some(t) = tokens.next() {
        match t {
            // `#[attr]` / `#![attr]`: skip the bracket group.
            TokenTree::Punct(p) if p.as_char() == '#' => {
                if matches!(tokens.peek(), Some(TokenTree::Punct(b)) if b.as_char() == '!') {
                    tokens.next();
                }
                tokens.next();
            }
            TokenTree::Ident(i) if i == "unsafe" || i == "default" => {}
            TokenTree::Ident(i) => return i == "impl",
            _ => return false,
        }
    }
    false
}

/// `item` (an `impl` whose body does not parse) rebuilt from what does: its header, and each
/// item of its body that parses on its own; a method whose body is broken keeps its signature
/// with a diverging placeholder body. `None` when even the header does not parse.
fn recover_impl(item: &TokenStream2) -> Option<ItemImpl> {
    use proc_macro2::{Delimiter, Group, TokenTree};
    let mut tokens: Vec<TokenTree> = item.clone().into_iter().collect();
    let body_at = tokens.iter().rposition(|t| matches!(t, TokenTree::Group(g) if g.delimiter() == Delimiter::Brace))?;
    let TokenTree::Group(body) = tokens[body_at].clone() else { return None };
    tokens[body_at] = TokenTree::Group(Group::new(Delimiter::Brace, TokenStream2::new()));
    tokens.truncate(body_at + 1);
    let mut imp: ItemImpl = syn::parse2(tokens.into_iter().collect()).ok()?;
    for chunk in split_items(body.stream()) {
        if let Ok(parsed) = syn::parse2::<ImplItem>(chunk.clone()) {
            imp.items.push(parsed);
            continue;
        }
        // A method whose body is broken: its signature, with a body that type-checks as anything.
        let mut sig: Vec<TokenTree> = chunk.into_iter().collect();
        if matches!(sig.last(), Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Brace) {
            sig.pop();
            let mut stub: TokenStream2 = sig.into_iter().collect();
            stub.extend(quote!({ ::core::unreachable!() }));
            if let Ok(parsed) = syn::parse2::<ImplItem>(stub) {
                imp.items.push(parsed);
            }
        }
    }
    Some(imp)
}

/// Splits the tokens of an `impl` body into its items: each ends after a top-level `{ … }`
/// (a method body; a `;` right after it belongs to it) or a top-level `;`.
fn split_items(body: TokenStream2) -> Vec<TokenStream2> {
    use proc_macro2::{Delimiter, TokenTree};
    let mut items = Vec::new();
    let mut current: Vec<TokenTree> = Vec::new();
    let mut tokens = body.into_iter().peekable();
    while let Some(t) = tokens.next() {
        let ends = match &t {
            TokenTree::Group(g) => g.delimiter() == Delimiter::Brace,
            TokenTree::Punct(p) => p.as_char() == ';',
            _ => false,
        };
        current.push(t);
        if ends {
            if matches!(tokens.peek(), Some(TokenTree::Punct(p)) if p.as_char() == ';') {
                if let Some(semi) = tokens.next() {
                    current.push(semi);
                }
            }
            items.push(std::mem::take(&mut current).into_iter().collect());
        }
    }
    if !current.is_empty() {
        items.push(current.into_iter().collect());
    }
    items
}

/// The methods `#[event_handlers]` treats as handlers (a `self` receiver, or `async`, and no
/// `#[handler(skip)]`, already removed or not).
fn is_handler_candidate(f: &ImplItemFn) -> bool {
    f.sig.receiver().is_some() || f.sig.asyncness.is_some()
}

/// A handler's signature is fixed by its event, like a WinForms handler's `(object sender,
/// EventArgs e)`: an unused `sender` or `e` is not a mistake and must not be flagged (by rustc or
/// rust-analyzer). Rather than `#[allow(unused_variables)]` on the method — which would also hide
/// a genuinely unused `let` in its body — each named parameter is touched once at the top of the
/// body (`let _ = &e;`, spanned at the parameter), so only the parameters are exempt. Called once
/// per handler method, whether or not its signature is valid (a mistake in one handler must not
/// also flag its parameters).
fn mark_params_used(f: &mut ImplItemFn) {
    let mut touches: Vec<syn::Stmt> = Vec::new();
    for input in &f.sig.inputs {
        let FnArg::Typed(t) = input else { continue };
        if let syn::Pat::Ident(p) = &*t.pat {
            if p.ident.to_string().starts_with('_') {
                continue;
            }
            let ident = &p.ident;
            touches.push(syn::parse_quote_spanned!(ident.span()=> let _ = &#ident;));
        }
    }
    if touches.is_empty() {
        return;
    }
    touches.append(&mut f.block.stmts);
    f.block.stmts = touches;
}

/// How a handler takes its sender.
enum SenderParam {
    /// `&Sender<C>`: the control type `C`.
    Typed(Box<Type>),
    /// `&ElementRef`.
    Element,
}

/// How a handler takes the event args.
enum ArgsParam {
    /// `&A` / `&mut A`.
    Typed { ty: Box<Type>, mutable: bool },
    /// `&dyn EventArgs` / `&mut dyn EventArgs`.
    Dyn { mutable: bool },
}

struct Handler {
    name: LitStr,
    method: syn::Ident,
    sender: Option<SenderParam>,
    args: Option<ArgsParam>,
    /// `Some` for an `async fn` handler (EVT-6); `sender`/`args` are then `None`.
    asynchronous: Option<AsyncShape>,
}

/// The parameters of an `async fn` handler: `(ui: UiHandle<Self>, e: A)`, each optional.
struct AsyncShape {
    ui: bool,
    /// The args type, taken by value (a copy).
    args: Option<Box<Type>>,
}

fn expand_impl(attr: TokenStream2, imp: &mut ItemImpl) -> syn::Result<TokenStream2> {
    if !attr.is_empty() {
        return Err(syn::Error::new_spanned(attr, "`#[event_handlers]` takes no arguments"));
    }
    if let Some((_, path, _)) = &imp.trait_ {
        return Err(syn::Error::new_spanned(
            path,
            "`#[event_handlers]` goes on an inherent `impl` of the view model (`impl MainViewModel { … }`), not on a trait `impl`",
        ));
    }

    let mut handlers: Vec<Handler> = Vec::new();
    let mut errors: Option<syn::Error> = None;
    let mut push_error = |e: syn::Error| match &mut errors {
        Some(all) => all.combine(e),
        None => errors = Some(e),
    };

    for item in &mut imp.items {
        let ImplItem::Fn(f) = item else { continue };
        let options = match take_handler_options(f) {
            Ok(o) => o,
            Err(e) => {
                push_error(e);
                continue;
            }
        };
        if options.skip || !is_handler_candidate(f) {
            continue; // Not a handler: a helper method, or an associated fn (`new()`).
        }
        // Handlers are called through the generated `match`, often ignoring their sender or
        // args (the designer's stub takes both), like WinForms handlers.
        mark_params_used(f);
        let parsed = if f.sig.asyncness.is_some() { parse_async_handler(f, options.name) } else { parse_handler(f, options.name) };
        match parsed {
            Ok(h) => {
                if let Some(first) = handlers.iter().find(|o| o.name.value() == h.name.value()) {
                    push_error(syn::Error::new(
                        h.name.span(),
                        format!(
                            "two handlers are named `{}` (the methods `{}` and `{}`): rename one, or give it another name with `#[handler(name = \"…\")]`",
                            h.name.value(),
                            first.method,
                            h.method
                        ),
                    ));
                } else {
                    handlers.push(h);
                }
            }
            Err(e) => push_error(e),
        }
    }
    if let Some(e) = errors {
        return Err(e);
    }

    let arms: Vec<TokenStream2> = handlers.iter().map(dispatch_arm).collect();
    let infos: Vec<TokenStream2> = handlers.iter().map(handler_info).collect();
    let sink = sink_impl(imp, &arms, &infos);
    Ok(quote!(#imp #sink))
}

fn sink_impl(imp: &ItemImpl, arms: &[TokenStream2], infos: &[TokenStream2]) -> TokenStream2 {
    let ev = quote!(::kubuno_views::events);
    let self_ty = &imp.self_ty;
    let (impl_g, _, where_g) = imp.generics.split_for_impl();
    quote! {
        impl #impl_g #ev::EventSink for #self_ty #where_g {
            const HANDLERS: &'static [#ev::HandlerInfo] = &[#(#infos),*];

            #[allow(unused_variables, clippy::needless_borrow)]
            fn handle_event(&mut self, handler: &str, cx: &#ev::HandlerContext<'_>, args: &mut dyn #ev::EventArgs) -> bool {
                match handler {
                    #(#arms)*
                    _ => false,
                }
            }
        }
    }
}

#[derive(Default)]
struct HandlerOptions {
    skip: bool,
    name: Option<LitStr>,
}

/// Reads and removes the method's `#[handler(…)]` attributes.
fn take_handler_options(f: &mut ImplItemFn) -> syn::Result<HandlerOptions> {
    let mut options = HandlerOptions::default();
    let mut result = Ok(());
    f.attrs.retain(|attr| {
        if !attr.path().is_ident("handler") {
            return true;
        }
        let parsed = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip") {
                options.skip = true;
                Ok(())
            } else if meta.path.is_ident("name") {
                let lit: LitStr = meta.value()?.parse()?;
                if lit.value().is_empty() {
                    return Err(syn::Error::new(lit.span(), "a handler name cannot be empty"));
                }
                options.name = Some(lit);
                Ok(())
            } else {
                Err(meta.error("unknown `handler` option (expected `skip` or `name = \"…\"`)"))
            }
        });
        if let Err(e) = parsed {
            result = Err(e);
        }
        false
    });
    result.map(|()| options)
}

fn strip_helper_attributes(imp: &mut ItemImpl) {
    for item in &mut imp.items {
        if let ImplItem::Fn(f) = item {
            f.attrs.retain(|a| !a.path().is_ident("handler"));
        }
    }
}

const SHAPE: &str = "fn name(&mut self, sender: &Sender<Button>, e: &MouseEventArgs)";

fn parse_handler(f: &ImplItemFn, name: Option<LitStr>) -> syn::Result<Handler> {
    let sig = &f.sig;
    let method = sig.ident.clone();
    let name = name.unwrap_or_else(|| LitStr::new(method.to_string().trim_start_matches("r#"), method.span()));

    if !sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&sig.generics, "a handler cannot be generic: it is called by name for one event"));
    }
    if let ReturnType::Type(_, ty) = &sig.output {
        let unit = matches!(&**ty, Type::Tuple(t) if t.elems.is_empty());
        if !unit {
            return Err(syn::Error::new_spanned(ty, "a handler returns nothing: report the outcome through its args (`e.handled = true`, `e.cancel = true`) or the view model"));
        }
    }

    let mut inputs = sig.inputs.iter();
    if let Some(FnArg::Receiver(r)) = inputs.next() {
        if r.reference.is_none() || r.colon_token.is_some() {
            return Err(syn::Error::new_spanned(r, format!("a handler takes `&mut self` (or `&self`): `{SHAPE}`")));
        }
    }
    let params: Vec<&syn::PatType> = inputs
        .filter_map(|a| match a {
            FnArg::Typed(t) => Some(t),
            FnArg::Receiver(_) => None, // Only the first parameter can be a receiver.
        })
        .collect();
    if params.len() > 2 {
        return Err(syn::Error::new_spanned(
            &params[2].ty,
            format!("a handler takes at most its sender and the event args: `{SHAPE}`"),
        ));
    }

    let mut sender = None;
    let mut args = None;
    for (i, p) in params.iter().enumerate() {
        match classify(&p.ty)? {
            Param::Sender(s) => {
                if i == 1 || sender.is_some() {
                    return Err(syn::Error::new_spanned(&p.ty, format!("the sender comes first, then the event args: `{SHAPE}`")));
                }
                sender = Some(s);
            }
            Param::Args(a) => {
                if args.is_some() {
                    return Err(syn::Error::new_spanned(&p.ty, format!("a handler takes one args parameter; the other one is the sender: `{SHAPE}`")));
                }
                args = Some(a);
            }
        }
    }
    Ok(Handler { name, method, sender, args, asynchronous: None })
}

const ASYNC_SHAPE: &str = "async fn name(ui: UiHandle<Self>, e: MouseEventArgs)";

/// An `async fn` handler (`vskubuno/docs/EVENTS.md` §6, EVT-6): no receiver (a borrow of the
/// view model cannot live across `.await`), an optional `UiHandle<Self>` then an optional copy
/// of the args. `handled`/`cancel` cannot be set from it: `&mut` args are refused here, and
/// writable args by value by the `ReadOnlyArgs` bound of the generated code.
fn parse_async_handler(f: &ImplItemFn, name: Option<LitStr>) -> syn::Result<Handler> {
    let sig = &f.sig;
    let method = sig.ident.clone();
    let name = name.unwrap_or_else(|| LitStr::new(method.to_string().trim_start_matches("r#"), method.span()));

    if !sig.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(&sig.generics, "a handler cannot be generic: it is called by name for one event"));
    }
    if let ReturnType::Type(_, ty) = &sig.output {
        let unit = matches!(&**ty, Type::Tuple(t) if t.elems.is_empty());
        if !unit {
            return Err(syn::Error::new_spanned(
                ty,
                "an async handler returns nothing (an `async fn` helper that is not a handler needs `#[handler(skip)]`)",
            ));
        }
    }
    let mut ui = false;
    let mut args: Option<Box<Type>> = None;
    for input in &sig.inputs {
        let ty = match input {
            FnArg::Receiver(r) => {
                return Err(syn::Error::new_spanned(
                    r,
                    format!(
                        "an async handler cannot borrow the view model across `.await`: take `ui: UiHandle<Self>` instead of `self`, and change the view model with `ui.update(|vm| …)`: `{ASYNC_SHAPE}`"
                    ),
                ));
            }
            FnArg::Typed(t) => &t.ty,
        };
        if last_segment(ty).is_some_and(|s| s.ident == "UiHandle") {
            if ui || args.is_some() {
                return Err(syn::Error::new_spanned(ty, format!("the `UiHandle` comes first, once: `{ASYNC_SHAPE}`")));
            }
            ui = true;
            continue;
        }
        if is_sender_like(ty) || matches!(&**ty, Type::Reference(r) if is_sender_like(&r.elem)) {
            return Err(syn::Error::new_spanned(
                ty,
                format!("an async handler takes no sender (it outlives the event): read what you need in a synchronous handler, or from the view model: `{ASYNC_SHAPE}`"),
            ));
        }
        if let Type::Reference(r) = &**ty {
            let msg = if r.mutability.is_some() {
                format!(
                    "an async handler cannot set `handled`/`cancel`: the event is over when it resumes after an `.await`. Set them in a synchronous handler (`fn f(&mut self, e: &mut …)`) and start the async work from it with `spawn_local`; an async handler takes a copy of the args: `{ASYNC_SHAPE}`"
                )
            } else {
                format!("an async handler takes a copy of the event args, not a reference (it outlives the event): `{ASYNC_SHAPE}`")
            };
            return Err(syn::Error::new_spanned(ty, msg));
        }
        if matches!(&**ty, Type::TraitObject(_) | Type::ImplTrait(_)) {
            return Err(syn::Error::new_spanned(ty, format!("an async handler takes a concrete args type, by value: `{ASYNC_SHAPE}`")));
        }
        if args.is_some() {
            return Err(syn::Error::new_spanned(ty, format!("an async handler takes at most a `UiHandle` and the event args: `{ASYNC_SHAPE}`")));
        }
        args = Some(ty.clone());
    }
    Ok(Handler { name, method, sender: None, args: None, asynchronous: Some(AsyncShape { ui, args }) })
}

/// The dispatch arm of an async handler: copies the args, takes the view's `UiHandle`, and
/// spawns the handler's future on the UI thread's executor.
fn async_arm(h: &Handler, shape: &AsyncShape) -> TokenStream2 {
    let ev = quote!(::kubuno_views::events);
    let name = &h.name;
    let method = &h.method;
    let (args_let, args_arg) = match &shape.args {
        Some(ty) => (
            quote_spanned! {ty.span()=>
                let __kv_e: #ty = match #ev::typed::with_args::<#ty, _>(args, #name, |__kv_e| #ev::typed::copy_for_async::<#ty>(__kv_e)) {
                    ::core::option::Option::Some(e) => e,
                    ::core::option::Option::None => return true,
                };
            },
            quote!(__kv_e),
        ),
        None => (TokenStream2::new(), TokenStream2::new()),
    };
    let (ui_let, ui_arg) = if shape.ui {
        (
            quote! {
                let ::core::option::Option::Some(__kv_ui) = #ev::UiHandle::<Self>::current() else {
                    #ev::executor::no_view_for(#name);
                    return true;
                };
            },
            if shape.args.is_some() { quote!(__kv_ui,) } else { quote!(__kv_ui) },
        )
    } else {
        (TokenStream2::new(), TokenStream2::new())
    };
    quote! {
        #name => {
            #args_let
            #ui_let
            #ev::executor::spawn_handler(#name, Self::#method(#ui_arg #args_arg));
            true
        }
    }
}

enum Param {
    Sender(SenderParam),
    Args(ArgsParam),
}

/// The last path segment's name of `ty` (`Sender` for `kubuno_views::events::Sender<'_, X>`).
fn last_segment(ty: &Type) -> Option<&syn::PathSegment> {
    match ty {
        Type::Path(p) if p.qself.is_none() => p.path.segments.last(),
        Type::Group(g) => last_segment(&g.elem),
        Type::Paren(p) => last_segment(&p.elem),
        _ => None,
    }
}

fn is_sender_like(ty: &Type) -> bool {
    last_segment(ty).is_some_and(|s| s.ident == "Sender" || s.ident == "ElementRef")
}

fn classify(ty: &Type) -> syn::Result<Param> {
    let Type::Reference(TypeReference { mutability, elem, .. }) = ty else {
        return Err(if is_sender_like(ty) {
            syn::Error::new_spanned(ty, "take the sender by reference: `sender: &Sender<Button>` (or `&ElementRef`)")
        } else {
            syn::Error::new_spanned(
                ty,
                "take the event args by reference: `e: &MouseEventArgs`, or `e: &mut MouseEventArgs` to set `handled`/`cancel`",
            )
        });
    };
    let mutable = mutability.is_some();

    if let Some(seg) = last_segment(elem) {
        if seg.ident == "Sender" || seg.ident == "ElementRef" {
            if mutable {
                return Err(syn::Error::new_spanned(ty, "the sender is read-only: `sender: &Sender<…>` (act on controls through the view model)"));
            }
            if seg.ident == "ElementRef" {
                return Ok(Param::Sender(SenderParam::Element));
            }
            let component = match &seg.arguments {
                PathArguments::AngleBracketed(a) => a.args.iter().rev().find_map(|g| match g {
                    GenericArgument::Type(t) => Some(t.clone()),
                    _ => None,
                }),
                _ => None,
            };
            return match component {
                Some(c) => Ok(Param::Sender(SenderParam::Typed(Box::new(c)))),
                None => Err(syn::Error::new_spanned(seg, "`Sender` needs the control type: `sender: &Sender<Button>` (`Sender<AnyElement>` for any element)")),
            };
        }
    }
    if let Type::TraitObject(obj) = &**elem {
        let is_event_args = obj.bounds.iter().any(|b| matches!(b, syn::TypeParamBound::Trait(t) if t.path.segments.last().is_some_and(|s| s.ident == "EventArgs")));
        if is_event_args {
            return Ok(Param::Args(ArgsParam::Dyn { mutable }));
        }
        return Err(syn::Error::new_spanned(elem, "the only trait object a handler takes is `&dyn EventArgs` (any event's args)"));
    }
    Ok(Param::Args(ArgsParam::Typed { ty: elem.clone(), mutable }))
}

fn dispatch_arm(h: &Handler) -> TokenStream2 {
    if let Some(shape) = &h.asynchronous {
        return async_arm(h, shape);
    }
    let ev = quote!(::kubuno_views::events);
    let name = &h.name;
    let method = &h.method;
    let (sender_let, sender_arg) = match &h.sender {
        Some(SenderParam::Typed(c)) => (
            quote_spanned! {c.span()=>
                let __kv_sender = match cx.typed_sender::<#c>(#name) {
                    ::core::option::Option::Some(sender) => sender,
                    ::core::option::Option::None => return true,
                };
            },
            quote!(&__kv_sender,),
        ),
        Some(SenderParam::Element) => (quote!(let __kv_sender = cx.element();), quote!(__kv_sender,)),
        None => (TokenStream2::new(), TokenStream2::new()),
    };
    let call = match &h.args {
        None => quote!(self.#method(#sender_arg);),
        Some(ArgsParam::Dyn { mutable: true }) => quote!(self.#method(#sender_arg args);),
        Some(ArgsParam::Dyn { mutable: false }) => quote!(self.#method(#sender_arg &*args);),
        Some(ArgsParam::Typed { ty, mutable }) => {
            let e = if *mutable { quote!(__kv_e) } else { quote!(&*__kv_e) };
            quote_spanned! {ty.span()=>
                let _ = #ev::typed::with_args::<#ty, _>(args, #name, |__kv_e| self.#method(#sender_arg #e));
            }
        }
    };
    quote! {
        #name => {
            #sender_let
            #call
            true
        }
    }
}

fn handler_info(h: &Handler) -> TokenStream2 {
    let ev = quote!(::kubuno_views::events);
    let name = &h.name;
    let method = LitStr::new(h.method.to_string().trim_start_matches("r#"), h.method.span());
    // Built with `HandlerInfo`'s const builders, never a struct literal: a field added to the
    // runtime type later must not break code this (possibly stale) macro build expands.
    let sender = match &h.sender {
        Some(SenderParam::Typed(c)) => quote_spanned!(c.span()=> .with_sender(<#c as #ev::ElementType>::ELEMENT)),
        Some(SenderParam::Element) => quote!(.with_sender("*")),
        None => TokenStream2::new(),
    };
    let args = match (&h.args, &h.asynchronous) {
        (_, Some(AsyncShape { args: Some(ty), .. })) => quote_spanned!(ty.span()=> .with_args(<#ty as #ev::ArgsChain>::NAME, false)),
        (Some(ArgsParam::Typed { ty, mutable }), _) => quote_spanned!(ty.span()=> .with_args(<#ty as #ev::ArgsChain>::NAME, #mutable)),
        (Some(ArgsParam::Dyn { mutable }), _) => quote!(.with_args("EventArgs", #mutable)),
        _ => TokenStream2::new(),
    };
    let asynchronous = if h.asynchronous.is_some() { quote!(.asynchronous()) } else { TokenStream2::new() };
    quote! {
        #ev::HandlerInfo::new(#name, #method) #sender #args #asynchronous
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn error_of(item: TokenStream2) -> String {
        let mut imp: ItemImpl = syn::parse2(item).expect("an impl");
        match expand_impl(TokenStream2::new(), &mut imp) {
            Ok(_) => panic!("expected an error"),
            Err(e) => e.to_string(),
        }
    }

    #[test]
    fn a_valid_impl_expands_to_a_sink_with_one_arm_per_handler() {
        let mut imp: ItemImpl = syn::parse_quote! {
            impl Vm {
                fn ok_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) {}
                fn key(&mut self, e: &mut KeyEventArgs) {}
                fn any(&self, e: &dyn EventArgs) {}
                fn plain(&mut self) {}
                fn from(&mut self, sender: &ElementRef<'_>) {}
                #[handler(name = "other")] fn renamed(&mut self) {}
                #[handler(skip)] fn helper(&mut self, x: u32) -> u32 { x }
                fn new() -> Self { Vm }
            }
        };
        let out = expand_impl(TokenStream2::new(), &mut imp).expect("expands").to_string();
        for name in ["\"ok_click\"", "\"key\"", "\"any\"", "\"plain\"", "\"from\"", "\"other\""] {
            assert!(out.contains(&format!("{name} =>")), "{name} arm missing: {out}");
        }
        assert!(!out.contains("\"helper\" =>") && !out.contains("\"new\" =>"));
        assert!(out.contains("EventSink for Vm"));
        assert!(out.contains("typed_sender :: < Button >"), "{out}");
        assert!(out.contains("with_args :: < MouseEventArgs , _ >"), "{out}");
        assert!(!out.contains("# [handler"), "helper attributes are removed: {out}");
    }

    #[test]
    fn wrong_signatures_have_clear_errors() {
        let cases: [(TokenStream2, &str); 11] = [
            (quote!(impl Vm { async fn a(&mut self) {} }), "an async handler cannot borrow the view model across `.await`"),
            (quote!(impl Vm { fn a(&mut self, e: MouseEventArgs) {} }), "take the event args by reference"),
            (quote!(impl Vm { fn a(&mut self, s: Sender<Button>) {} }), "take the sender by reference"),
            (quote!(impl Vm { fn a(&mut self, s: &mut Sender<Button>) {} }), "the sender is read-only"),
            (quote!(impl Vm { fn a(&mut self, s: &Sender) {} }), "`Sender` needs the control type"),
            (quote!(impl Vm { fn a(&mut self, e: &MouseEventArgs, s: &Sender<Button>) {} }), "the sender comes first"),
            (quote!(impl Vm { fn a(&mut self, s: &Sender<Button>, e: &MouseEventArgs, x: &u8) {} }), "at most its sender and the event args"),
            (quote!(impl Vm { fn a(self) {} }), "takes `&mut self` (or `&self`)"),
            (quote!(impl Vm { fn a(&mut self) -> bool { true } }), "a handler returns nothing"),
            (quote!(impl Vm { fn a<T>(&mut self) {} }), "cannot be generic"),
            (quote!(impl Vm { fn a(&mut self) {} #[handler(name = "a")] fn b(&mut self) {} }), "two handlers are named `a`"),
        ];
        for (item, expected) in cases {
            let err = error_of(item.clone());
            assert!(err.contains(expected), "{item}: expected `{expected}`, got `{err}`");
        }
    }

    #[test]
    fn async_handlers_spawn_a_task_with_a_ui_handle_and_a_copy_of_the_args() {
        let mut imp: ItemImpl = syn::parse_quote! {
            impl Vm {
                async fn refresh_click(ui: UiHandle<Self>, e: MouseEventArgs) {}
                async fn tick(ui: UiHandle<Self>) {}
                async fn bare() {}
                #[handler(skip)] async fn fetch(url: String) -> String { url }
            }
        };
        let out = expand_impl(TokenStream2::new(), &mut imp).expect("expands").to_string();
        for name in ["\"refresh_click\"", "\"tick\"", "\"bare\""] {
            assert!(out.contains(&format!("{name} =>")), "{name} arm missing: {out}");
        }
        assert!(!out.contains("\"fetch\" =>"));
        assert!(out.contains("copy_for_async :: < MouseEventArgs >"), "{out}");
        assert!(out.contains("UiHandle :: < Self > :: current ()"), "{out}");
        assert!(out.contains("spawn_handler (\"refresh_click\" , Self :: refresh_click (__kv_ui , __kv_e))"), "{out}");
        assert!(out.contains("spawn_handler (\"tick\" , Self :: tick (__kv_ui))"), "{out}");
        assert!(out.contains("spawn_handler (\"bare\" , Self :: bare ())"), "{out}");
        assert!(out.contains(". asynchronous ()"), "{out}");
        assert!(!out.contains("HandlerInfo {"), "HandlerInfo is built with its const builders, never a struct literal: {out}");
    }

    #[test]
    fn only_handler_parameters_are_exempt_from_the_unused_lint() {
        let mut imp: ItemImpl = syn::parse_quote! {
            impl Vm {
                fn ok_click(&mut self, sender: &Sender<Button>, e: &MouseEventArgs) { let body_local = 1; }
                fn quiet(&mut self, _e: &MouseEventArgs) {}
                #[handler(skip)] fn helper(&self, x: u32) {}
                async fn later(ui: UiHandle<Self>) {}
            }
        };
        let out = expand_impl(TokenStream2::new(), &mut imp).expect("expands").to_string();
        assert!(out.contains("let _ = & sender ; let _ = & e ; let body_local = 1 ;"), "{out}");
        assert!(out.contains("let _ = & ui ;"), "{out}");
        assert!(!out.contains("& _e") && !out.contains("& x ;"), "`_e` and a skipped helper are left alone: {out}");
        assert!(!out.contains("# [allow (unused_variables)]"), "no blanket allow on the methods, which would also hide unused locals: {out}");
    }

    #[test]
    fn a_syntax_error_inside_the_impl_is_reported_where_it_is_and_the_input_kept() {
        let item = quote!(impl Vm { fn ok_click(&mut self, e: &MouseEventArgs) { let x = ; } const N: u32 = { 1 }; fn other(&mut self) {} });
        let out = expand(TokenStream2::new(), item).to_string();
        assert!(!out.contains("compile_error"), "the compiler reports the syntax error itself, once: {out}");
        assert!(!out.contains("goes on the `impl` block"), "a syntax error is not a misplaced attribute: {out}");
        // The broken method keeps its signature (a placeholder body), the rest is expanded as usual.
        assert!(out.contains("fn ok_click (& mut self , e : & MouseEventArgs) { let _ = & e ; :: core :: unreachable ! () }"), "{out}");
        assert!(out.contains("const N : u32 = { 1 } ;"), "{out}");
        assert!(out.contains("\"other\" =>") && out.contains("\"ok_click\" =>"), "the sink still dispatches every handler: {out}");
        assert_eq!(split_items(quote!(fn a() {} const B: u8 = { 1 }; type C = u8; fn d(&self) { x })).len(), 4);
        let item = quote!(#[doc = "x"] unsafe impl Vm { fn a(&mut self) { let = ; } });
        assert!(looks_like_impl(&item));
        assert!(!looks_like_impl(&quote!(fn f() {})));
    }

    #[test]
    fn async_handlers_cannot_set_handled_or_cancel_nor_borrow() {
        let cases: [(TokenStream2, &str); 8] = [
            (quote!(impl Vm { async fn a(ui: UiHandle<Self>, e: &mut FormClosingEventArgs) {} }), "an async handler cannot set `handled`/`cancel`"),
            (quote!(impl Vm { async fn a(e: &MouseEventArgs) {} }), "takes a copy of the event args, not a reference"),
            (quote!(impl Vm { async fn a(&self) {} }), "cannot borrow the view model across `.await`"),
            (quote!(impl Vm { async fn a(s: &Sender<Button>) {} }), "takes no sender"),
            (quote!(impl Vm { async fn a(e: MouseEventArgs, ui: UiHandle<Self>) {} }), "the `UiHandle` comes first"),
            (quote!(impl Vm { async fn a(ui: UiHandle<Self>, e: MouseEventArgs, f: KeyEventArgs) {} }), "at most a `UiHandle` and the event args"),
            (quote!(impl Vm { async fn b(e: &dyn EventArgs) {} }), "not a reference"),
            (quote!(impl Vm { async fn a() -> u32 { 1 } }), "`#[handler(skip)]`"),
        ];
        for (item, expected) in cases {
            let err = error_of(item.clone());
            assert!(err.contains(expected), "{item}: expected `{expected}`, got `{err}`");
        }
    }

    #[test]
    fn attribute_misuse_has_clear_errors() {
        assert!(error_of(quote!(impl Handlers for Vm { fn a(&mut self) {} })).contains("not on a trait `impl`"));
        assert!(error_of(quote!(impl Vm { #[handler(rename = "x")] fn a(&mut self) {} })).contains("unknown `handler` option"));
        assert!(error_of(quote!(impl Vm { fn a(&mut self, e: &dyn Debug) {} })).contains("`&dyn EventArgs`"));
        let mut imp: ItemImpl = syn::parse_quote!(impl Vm {});
        assert!(expand_impl(quote!(x), &mut imp).unwrap_err().to_string().contains("takes no arguments"));
        let out = expand(TokenStream2::new(), quote!(fn not_an_impl() {})).to_string();
        assert!(out.contains("goes on the `impl` block"), "{out}");
    }

    #[test]
    fn an_error_still_emits_the_impl_and_an_empty_sink() {
        let out = expand(TokenStream2::new(), quote!(impl Vm { #[handler(skip)] fn ok(&self) {} async fn bad(&mut self) {} })).to_string();
        assert!(out.contains("impl Vm"), "{out}");
        assert!(out.contains("EventSink for Vm"), "{out}");
        assert!(out.contains("compile_error"), "{out}");
        assert!(!out.contains("# [handler"), "{out}");
    }
}
