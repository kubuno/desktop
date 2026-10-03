//! `#[kubuno::view("main_view.kbview")]`: a Windows Forms-like form class over a `.kbview`.
//!
//! The attribute reads the view at compile time (tracked with `include_str!`, so editing the view
//! rebuilds the crate) and generates, without writing any file — the `.kbview` stays the single
//! source of truth, like a `Form1.Designer.cs` nobody edits:
//!
//! - one field per `x:Name`d element, typed with the control's handle (`status: kubuno::TextField`),
//!   and a hidden `kubuno::Form` field;
//! - `initialize_component()`, which links those fields to the view (`InitializeComponent()`);
//! - `kubuno::View` (the form, and a `match` from each handler name the view uses to the struct's
//!   method of that name — plain methods, no attribute needed), `ViewModel` (the controls'
//!   properties, `#[bind]` fields for `{Binding …}` paths, a `#[data_context]` field for the rest),
//!   `Deref`/`DerefMut` to `kubuno::Form` (`self.set_text("Title")`, `self.close()`), `Default`
//!   (unless derived), and `show()` / `show_dialog(owner)`.

use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote, quote_spanned};
use syn::parse::{Parse, ParseStream, Parser};
use syn::spanned::Spanned;
use syn::{Fields, Ident, ItemStruct, LitStr, Token};

use kubuno_views_meta::kbview::{self, KbElement, BUILTIN_ELEMENTS, LIBRARY_ELEMENTS, PRINT_TYPED, STORAGE_TYPED, TYPED_CONTROLS};

/// The attribute's arguments: `"path.kbview"`, `path = "…"` or `xml = "…"` (inline, for tests).
struct ViewArgs {
    path: Option<LitStr>,
    xml: Option<LitStr>,
}

impl Parse for ViewArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(LitStr) {
            let path: LitStr = input.parse()?;
            if !input.is_empty() {
                return Err(input.error("expected only the view's path: `#[kubuno::view(\"main_view.kbview\")]`"));
            }
            return Ok(Self { path: Some(path), xml: None });
        }
        let mut args = Self { path: None, xml: None };
        while !input.is_empty() {
            let key: Ident = input.parse()?;
            input.parse::<Token![=]>()?;
            let value: LitStr = input.parse()?;
            match key.to_string().as_str() {
                "path" => args.path = Some(value),
                "xml" => args.xml = Some(value),
                other => return Err(syn::Error::new(key.span(), format!("unknown option `{other}`: expected `\"file.kbview\"`, `path = \"…\"` or `xml = \"…\"`"))),
            }
            if !input.is_empty() {
                input.parse::<Token![,]>()?;
            }
        }
        match (&args.path, &args.xml) {
            (None, None) => Err(input.error("the view is missing: `#[kubuno::view(\"main_view.kbview\")]`")),
            (Some(_), Some(x)) => Err(syn::Error::new(x.span(), "give either the view's path or its `xml`, not both")),
            _ => Ok(args),
        }
    }
}

/// Where the view comes from.
struct Source {
    /// The file (absolute), or `None` for inline XML.
    file: Option<std::path::PathBuf>,
    /// The name shown in messages (`main_view.kbview`).
    display: String,
    text: String,
    span: Span,
}

fn err(span: Span, message: impl std::fmt::Display) -> TokenStream2 {
    syn::Error::new(span, message.to_string()).to_compile_error()
}

/// Resolves the view's path like `include_str!`: relative to the file holding the attribute, else
/// (tools that do not know that file) to the package's `src` folder, then to its root.
fn resolve(path: &LitStr) -> Result<std::path::PathBuf, String> {
    let rel = std::path::PathBuf::from(path.value());
    if rel.is_absolute() {
        return if rel.is_file() { Ok(rel) } else { Err(format!("cannot find the view `{}`", rel.display())) };
    }
    let mut tried = Vec::new();
    if let Some(dir) = proc_macro::Span::call_site().local_file().and_then(|f| f.parent().map(std::path::Path::to_path_buf)) {
        if dir.is_relative() {
            // Relative to the compiler's working directory (the workspace root under Cargo).
            if let Ok(cwd) = std::env::current_dir() {
                tried.push(cwd.join(&dir).join(&rel));
            }
            if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
                tried.push(std::path::PathBuf::from(manifest).join(&dir).join(&rel));
            }
        } else {
            tried.push(dir.join(&rel));
        }
    }
    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR").map(std::path::PathBuf::from) {
        tried.push(manifest.join("src").join(&rel));
        tried.push(manifest.join(&rel));
    }
    if let Some(found) = tried.iter().find(|p| p.is_file()) {
        return Ok(found.clone());
    }
    // A tool that expands the macro without telling which file holds it (rust-analyzer's
    // proc-macro server): the one file under `src` whose path ends with the given one.
    if let Some(src) = std::env::var_os("CARGO_MANIFEST_DIR").map(|m| std::path::PathBuf::from(m).join("src")) {
        let mut matches = Vec::new();
        let mut stack = vec![src];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else { continue };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.ends_with(&rel) {
                    matches.push(path);
                }
            }
        }
        if matches.len() == 1 {
            return Ok(matches.remove(0));
        }
    }
    tried.iter().find(|p| p.is_file()).cloned().ok_or_else(|| {
        format!(
            "cannot find the view `{}` (looked in {}): the path is relative to the Rust file that declares the view",
            path.value(),
            tried.iter().map(|p| format!("`{}`", p.display())).collect::<Vec<_>>().join(", ")
        )
    })
}

fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    chars.next().is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
        && syn::parse_str::<Ident>(s).is_ok()
}

/// `status_text` → `StatusText` (the binding path of a `#[bind]` field).
fn pascal(name: &str) -> String {
    name.split('_').filter(|p| !p.is_empty()).map(|p| {
        let mut c = p.chars();
        c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
    }).collect()
}

/// The custom control classes (`#[derive(Component)]`, `#[derive(UserControl)]`) declared in the
/// package's sources: what a non-built-in element of the view may name.
fn crate_classes() -> Vec<String> {
    let Some(root) = std::env::var_os("CARGO_MANIFEST_DIR").map(std::path::PathBuf::from) else { return Vec::new() };
    let mut out = Vec::new();
    // The file declaring the view too (a test, an example or a bench outside `src`).
    if proc_macro::is_available() {
        if let Some(file) = proc_macro::Span::call_site().local_file() {
            let file = if file.is_relative() { std::env::current_dir().map(|d| d.join(&file)).unwrap_or(file) } else { file };
            if let Ok(text) = std::fs::read_to_string(&file) {
                out.extend(kubuno_views_meta::scan_source(&text).components.into_iter().map(|c| c.name));
            }
        }
    }
    for path in package_rs_files(&root) {
        if let Ok(text) = std::fs::read_to_string(&path) {
            out.extend(kubuno_views_meta::scan_source(&text).components.into_iter().map(|c| c.name));
        }
    }
    out
}

/// The `.rs` files of the package in `root`, wherever they are (a control added next to `Cargo.toml` is declared
/// from the crate root with a `#[path]`): `target`, `obj`, `bin`, hidden folders and nested packages skipped.
fn package_rs_files(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if name != "target" && name != "obj" && name != "bin" && !name.starts_with('.') && !path.join("Cargo.toml").is_file() {
                    stack.push(path);
                }
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }
    out
}

/// The `[dependencies]` of `manifest` that are folders: `(extern crate name, folder)` — a
/// `path = "…"` dependency, or a `workspace = true` one whose `[workspace.dependencies]` entry (in
/// the workspace above `root`) has a path. The same reading as the language server's
/// (`kubuno-views-ls` `project::path_dependencies` / `workspace_dependencies`).
fn folder_dependencies(manifest: &str, root: &std::path::Path) -> Vec<(String, std::path::PathBuf)> {
    fn path_of(value: &str) -> Option<&str> {
        let pos = value.find("path")?;
        let rest = value[pos + 4..].trim_start().strip_prefix('=')?.trim_start().strip_prefix('"')?;
        rest.find('"').map(|end| &rest[..end])
    }
    let mut out = Vec::new();
    let mut from_workspace = Vec::new();
    let mut in_deps = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        let Some((key, value)) = line.split_once('=').filter(|_| in_deps) else { continue };
        let (key, value) = (key.trim().trim_matches('"'), value.trim());
        if let Some(name) = key.strip_suffix(".workspace") {
            if value.starts_with("true") {
                from_workspace.push(name.to_string());
            }
        } else if value.starts_with('{') && value.replace(' ', "").contains("workspace=true") {
            from_workspace.push(key.to_string());
        } else if let Some(path) = path_of(value) {
            out.push((key.replace('-', "_"), root.join(path)));
        }
    }
    if !from_workspace.is_empty() {
        let ws = root.ancestors().skip(1).find_map(|dir| {
            let text = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
            text.lines().any(|l| l.trim() == "[workspace]").then(|| (dir.to_path_buf(), text))
        });
        if let Some((ws_root, text)) = ws {
            let mut in_ws = false;
            for line in text.lines() {
                let line = line.trim();
                if line.starts_with('[') {
                    in_ws = line == "[workspace.dependencies]";
                    continue;
                }
                let Some((key, value)) = line.split_once('=').filter(|_| in_ws) else { continue };
                let key = key.trim().trim_matches('"');
                if from_workspace.iter().any(|n| n == key) {
                    if let Some(path) = path_of(value) {
                        out.push((key.replace('-', "_"), ws_root.join(path)));
                    }
                }
            }
        }
    }
    out
}

/// The custom control classes of the package's folder dependencies (a library of controls the
/// application depends on): `(class, extern crate name)`. The framework's own crates are skipped
/// (their elements are the built-in and library ones) — by their explicit list
/// ([`kubuno_views_meta::FRAMEWORK_CRATES`]), not by a `kubuno` prefix: a control library may be
/// named `kubuno-shell-controls` or `kubuno-acme-widgets`.
fn dependency_classes() -> Vec<(String, String)> {
    let Some(root) = std::env::var_os("CARGO_MANIFEST_DIR").map(std::path::PathBuf::from) else { return Vec::new() };
    dependency_classes_in(&root)
}

/// [`dependency_classes`] of the package at `root`.
fn dependency_classes_in(root: &std::path::Path) -> Vec<(String, String)> {
    let Ok(manifest) = std::fs::read_to_string(root.join("Cargo.toml")) else { return Vec::new() };
    let mut out = Vec::new();
    for (krate, dir) in folder_dependencies(&manifest, root) {
        if kubuno_views_meta::is_framework_crate(&krate) {
            continue;
        }
        for path in package_rs_files(&dir) {
            if let Ok(text) = std::fs::read_to_string(&path) {
                out.extend(kubuno_views_meta::scan_source(&text).components.into_iter().map(|c| (c.name, krate.clone())));
            }
        }
    }
    out
}

pub fn expand(attr: TokenStream2, item: TokenStream2) -> TokenStream2 {
    let args: ViewArgs = match syn::parse2(attr) {
        Ok(a) => a,
        Err(e) => return e.to_compile_error(),
    };
    let mut item: ItemStruct = match syn::parse2(item) {
        Ok(i) => i,
        Err(e) => return syn::Error::new(e.span(), "`#[kubuno::view]` goes on a struct: `pub struct MainView { … }`").to_compile_error(),
    };
    let source = match (&args.path, &args.xml) {
        (Some(path), _) => match resolve(path) {
            Ok(file) => match std::fs::read_to_string(&file) {
                Ok(text) => Source { display: path.value(), file: Some(file), text, span: path.span() },
                Err(e) => return err(path.span(), format!("cannot read the view `{}`: {e}", file.display())),
            },
            Err(message) => return err(path.span(), message),
        },
        (None, Some(xml)) => Source { file: None, display: "the inline view".to_string(), text: xml.value(), span: xml.span() },
        (None, None) => return err(Span::call_site(), "the view is missing"),
    };
    match expand_struct(&mut item, &source) {
        Ok(tokens) => tokens,
        Err(e) => {
            // The struct stays declared (so the rest of the file does not drown in unrelated errors).
            let mut out = e;
            out.extend(quote!(#item));
            out
        }
    }
}

fn expand_struct(item: &mut ItemStruct, source: &Source) -> Result<TokenStream2, TokenStream2> {
    let span = source.span;
    if !item.generics.params.is_empty() {
        return Err(err(item.generics.span(), "a view cannot be generic: its controls and handlers are those of one `.kbview`"));
    }
    // Visual inheritance (`x:Inherits="base_form.kbview"`, an inherited form): the view merged over its base view;
    // the derived file's own elements are told apart from the base's (`x:Inherited`).
    let own_elements = kbview::scan(&source.text).map_err(|e| err(span, format!("{}, {e}", source.display)))?;
    let inherited = match kubuno_views_meta::inherit::inherits(&source.text) {
        Some(_) => {
            let file = source.file.clone().or_else(|| std::env::var_os("CARGO_MANIFEST_DIR").map(|m| std::path::PathBuf::from(m).join("src").join("view.kbview")));
            match kubuno_views_meta::inherit::resolve(&source.text, file.as_deref(), &|p| std::fs::read_to_string(p)) {
                Ok(Some((merged, files))) => Some((merged, files)),
                Ok(None) => None,
                Err(message) => return Err(err(span, format!("{}: {message}", source.display))),
            }
        }
        None => None,
    };
    let view_text = inherited.as_ref().map(|(m, _)| m.clone()).unwrap_or_else(|| source.text.clone());
    let elements = kbview::scan(&view_text).map_err(|e| err(span, format!("{}, {e}", source.display)))?;
    let Some(root) = elements.first() else { return Err(err(span, format!("{} has no root element", source.display))) };
    if root.depth != 0 {
        return Err(err(span, format!("{} has no root element", source.display)));
    }
    // An element of the base view (`x:Inherited` set by the merge): `true`/`inner` not overridden, `override`.
    let from_base = |el: &KbElement| el.attribute(kubuno_views_meta::inherit::INHERITED_ATTRIBUTE).is_some_and(|v| v != "root");
    let overridable = |el: &KbElement| matches!(el.attribute("Modifiers"), Some("Public" | "Protected" | "Internal" | "ProtectedInternal"));

    // The user's fields: `#[bind]` / `#[data_context]` are read and removed.
    if matches!(item.fields, Fields::Unit) {
        item.fields = Fields::Named(syn::parse_quote!({}));
    }
    let Fields::Named(named) = &mut item.fields else {
        return Err(err(item.fields.span(), "`#[kubuno::view]` needs a struct with named fields (or none): `pub struct MainView { … }`"));
    };
    let mut user_fields = Vec::new();
    let mut binds = Vec::new();
    let mut data_context = None;
    // `#[control]` fields: declared by the user, typed as they want (`Custom<MessageThread>`),
    // linked to the element of their name.
    let mut control_fields: Vec<Ident> = Vec::new();
    // `#[base] base: BaseForm`: the form of the base view of an inherited view — its controls and handlers.
    let mut base_field: Option<Ident> = None;
    for field in named.named.iter_mut() {
        let Some(ident) = field.ident.clone() else { continue };
        let mut keep = Vec::new();
        for attr in std::mem::take(&mut field.attrs) {
            if attr.path().is_ident("base") {
                if inherited.is_none() {
                    return Err(err(attr.span(), "`#[base]` is the form of the base view: the view must inherit one (`x:Inherits=\"base_form.kbview\"` on its root)"));
                }
                if base_field.replace(ident.clone()).is_some() {
                    return Err(err(attr.span(), "a view has one `#[base]` field"));
                }
            } else if attr.path().is_ident("bind") {
                let path = match &attr.meta {
                    syn::Meta::Path(_) => pascal(&ident.to_string()),
                    syn::Meta::List(_) => match attr.parse_args::<LitStr>() {
                        Ok(p) => p.value(),
                        Err(_) => return Err(err(attr.span(), "expected `#[bind]` or `#[bind(\"BindingPath\")]`")),
                    },
                    syn::Meta::NameValue(_) => return Err(err(attr.span(), "expected `#[bind]` or `#[bind(\"BindingPath\")]`")),
                };
                binds.push((path, ident.clone(), field.ty.clone()));
            } else if attr.path().is_ident("control") {
                if !matches!(attr.meta, syn::Meta::Path(_)) {
                    return Err(err(attr.span(), "expected `#[control]`"));
                }
                control_fields.push(ident.clone());
            } else if attr.path().is_ident("data_context") {
                if data_context.is_some() {
                    return Err(err(attr.span(), "a view has one `#[data_context]` field"));
                }
                data_context = Some(ident.clone());
            } else {
                keep.push(attr);
            }
        }
        field.attrs = keep;
        user_fields.push((ident, field.ty.clone()));
    }

    // One field per `x:Name`d element.
    let classes = std::cell::OnceCell::new();
    let dependencies = std::cell::OnceCell::new();
    // The dependencies whose classes the view uses: referenced so they are linked (their classes
    // register at startup).
    let mut linked_crates: Vec<String> = Vec::new();
    let known = |n: &str| BUILTIN_ELEMENTS.contains(&n) || LIBRARY_ELEMENTS.contains(&n);
    let mut linked_crates_of: std::collections::HashMap<&str, String> = std::collections::HashMap::new();
    for el in &elements {
        if known(&el.name) || classes.get_or_init(crate_classes).contains(&el.name) {
            continue;
        }
        if let Some((_, krate)) = dependencies.get_or_init(dependency_classes).iter().find(|(c, _)| *c == el.name) {
            linked_crates_of.insert(el.name.as_str(), krate.clone());
            if !linked_crates.contains(krate) {
                linked_crates.push(krate.clone());
            }
        }
    }
    let mut members = Vec::new();
    for el in &elements {
        let Some(name) = el.x_name() else { continue };
        // A control of the base view: the `#[base]` form holds it; without one, a Protected/Public control gets its
        // field here (Windows Forms' inherited protected field), a private one none (it stays the base's).
        if from_base(el) && (base_field.is_some() || !overridable(el)) {
            continue;
        }
        if !is_identifier(name) {
            return Err(err(span, format!("{}, line {}: `x:Name=\"{name}\"` is not a Rust identifier (letters, digits and `_`, not a keyword): rename the control", source.display, el.line)));
        }
        if name.starts_with("__") {
            return Err(err(span, format!("{}, line {}: `x:Name=\"{name}\"`: names starting with `__` are reserved", source.display, el.line)));
        }
        if members.iter().any(|(n, _): &(String, &KbElement)| n == name) {
            return Err(err(span, format!("{}, line {}: two controls are named `{name}`", source.display, el.line)));
        }
        if user_fields.iter().any(|(f, _)| f == name) && !control_fields.iter().any(|f| f == name) {
            return Err(err(span, format!("{}, line {}: the control `{name}` has the name of a field of `{}`: rename one of them (or mark the field `#[control]` to type the control yourself)", source.display, el.line, item.ident)));
        }
        if !known(&el.name) && !classes.get_or_init(crate_classes).contains(&el.name) && !linked_crates_of.contains_key(el.name.as_str()) {
            return Err(err(
                span,
                format!(
                    "{}, line {}: unknown control `<{}>` (x:Name=\"{name}\"): it is neither a Kubuno control nor a `#[derive(Component)]` / `#[derive(UserControl)]` type of this crate or of a path dependency",
                    source.display, el.line, el.name
                ),
            ));
        }
        members.push((name.to_string(), el));
    }

    // Append the generated fields.
    let Fields::Named(named) = &mut item.fields else { unreachable!("checked above") };
    for f in &control_fields {
        if !members.iter().any(|(n, _)| f == n) {
            return Err(err(f.span(), format!("`#[control] {f}`: {} has no control named `{f}`", source.display)));
        }
    }
    for (name, el) in &members {
        if control_fields.iter().any(|f| f == name) {
            continue;
        }
        let ident = Ident::new(name, span);
        let ty = if TYPED_CONTROLS.contains(&el.name.as_str()) {
            let t = Ident::new(&el.name, span);
            quote_spanned!(span=> ::kubuno::forms::#t)
        } else if PRINT_TYPED.contains(&el.name.as_str()) {
            let t = Ident::new(&el.name, span);
            quote_spanned!(span=> ::kubuno::printing::#t)
        } else if STORAGE_TYPED.contains(&el.name.as_str()) {
            // `<Settings x:Name="settings">` → `settings: kubuno::storage::Settings` (vskubuno docs/STORAGE-COMPONENTS.md).
            let t = Ident::new(&el.name, span);
            quote_spanned!(span=> ::kubuno::storage::#t)
        } else {
            quote_spanned!(span=> ::kubuno::forms::Control)
        };
        let vis = match el.attribute("Modifiers") {
            Some("Public") => quote!(pub),
            Some("Internal") | Some("Protected") | Some("ProtectedInternal") => quote!(pub(crate)),
            _ => quote!(),
        };
        let doc = format!(" The `<{} x:Name=\"{name}\">` of {} (line {}), generated by `#[kubuno::view]`.", el.name, source.display, el.line);
        let field: syn::Field = syn::Field::parse_named.parse2(quote_spanned!(span=> #[doc = #doc] #vis #ident: #ty)).map_err(|e| e.to_compile_error())?;
        named.named.push(field);
    }
    let form_field: syn::Field = syn::Field::parse_named
        .parse2(quote!(#[doc(hidden)] pub __kubuno_form: ::kubuno::Form))
        .map_err(|e| e.to_compile_error())?;
    named.named.push(form_field);

    // Handler names the view uses (any element): the dispatch `match`.
    let mut handlers: Vec<String> = Vec::new();
    for el in &elements {
        for (_, h) in el.event_handlers() {
            if is_identifier(h) && !handlers.iter().any(|x| x == h) {
                handlers.push(h.to_string());
            }
        }
    }
    let name = &item.ident;
    // The handlers only the base view names run on the `#[base]` form (its methods, its controls).
    let own_handlers: Vec<&str> = own_elements.iter().flat_map(|el| el.event_handlers().map(|(_, h)| h)).collect();
    let arms = handlers.iter().map(|h| {
        let method = Ident::new(h, span);
        match &base_field {
            Some(base) if !own_handlers.contains(&h.as_str()) => quote_spanned!(span=> #h => ::kubuno::View::handle_event(&mut self.#base, #h, cx, args),),
            _ => quote_spanned!(span=> #h => ::kubuno::__private::call_handler(self, #name::#method, cx, args, #h),),
        }
    });
    let handler_names = handlers.iter();

    // Linking the fields.
    let links = members.iter().map(|(n, _)| {
        let ident = Ident::new(n, span);
        quote!((#n, ::kubuno::forms::AsControl::as_control(&self.#ident)))
    });

    // The view's source. An inherited view embeds the merged view, and tracks its own file and its bases (editing
    // any of them rebuilds the crate); it is not hot-reloaded from its file (the merge is the macro's).
    let text_expr = match (&source.file, &inherited) {
        (_, Some((merged, files))) => {
            let mut tracked: Vec<String> = files.iter().map(|f| f.to_string_lossy().into_owned()).collect();
            if let Some(file) = &source.file {
                tracked.push(file.to_string_lossy().into_owned());
            }
            quote!({ #(const _: &str = include_str!(#tracked);)* #merged })
        }
        (Some(file), None) => {
            let abs = file.to_string_lossy().into_owned();
            quote!(include_str!(#abs))
        }
        (None, None) => {
            let t = &source.text;
            quote!(#t)
        }
    };
    let path_expr = match &source.file {
        Some(_) if inherited.is_some() => quote!(::core::option::Option::None),
        Some(file) => {
            let abs = file.to_string_lossy().into_owned();
            quote!(::core::option::Option::Some(#abs))
        }
        None => quote!(::core::option::Option::None),
    };
    let display = &source.display;

    // `ViewModel`: controls, `#[bind]` fields, then the data context.
    let get_binds = binds.iter().map(|(path, ident, ty)| {
        quote_spanned!(ty.span()=> #path => return ::core::option::Option::Some(<#ty as ::kubuno::Bindable>::to_value(&self.#ident)),)
    });
    let set_binds = binds.iter().map(|(path, ident, ty)| {
        quote_spanned!(ty.span()=> #path => {
            if let ::core::option::Option::Some(v) = <#ty as ::kubuno::Bindable>::from_value(&value) {
                self.#ident = v;
            }
            return;
        })
    });
    let (dc_get, dc_set) = match &data_context {
        Some(dc) => (
            quote!(::kubuno::views::binding::ViewModel::get(&self.#dc, path)),
            quote!(::kubuno::views::binding::ViewModel::set(&mut self.#dc, path, value)),
        ),
        None => (quote!(::core::option::Option::None), quote!(let _ = value;)),
    };

    // `Default`, unless derived.
    let derives_default = item.attrs.iter().any(|a| {
        a.path().is_ident("derive") && {
            let mut found = false;
            let _ = a.parse_nested_meta(|m| {
                found |= m.path.segments.last().is_some_and(|s| s.ident == "Default");
                Ok(())
            });
            found
        }
    });
    let default_impl = if derives_default {
        quote!()
    } else {
        let user = user_fields.iter().map(|(f, ty)| quote_spanned!(ty.span()=> #f: <#ty as ::core::default::Default>::default(),));
        let gen = members.iter().filter(|(n, _)| !control_fields.iter().any(|f| f == n)).map(|(n, _)| {
            let ident = Ident::new(n, span);
            quote!(#ident: ::core::default::Default::default(),)
        });
        quote! {
            impl ::core::default::Default for #name {
                /// The view with its controls not yet linked: `new()` calls
                /// [`initialize_component`](Self::initialize_component) on it.
                fn default() -> Self {
                    Self { #(#user)* #(#gen)* __kubuno_form: ::core::default::Default::default() }
                }
            }
        }
    };
    let track = format_ident!("__KUBUNO_VIEW_OF_{}", name.to_string().to_uppercase());
    // The `#[base]` form shares this view's form, and lends its controls (an inherited form, its base's fields).
    let (base_form, base_members) = match &base_field {
        Some(base) => (
            quote!(self.#base.__kubuno_form = ::core::clone::Clone::clone(&self.__kubuno_form);),
            quote!(members.extend(self.#base.__kubuno_members());),
        ),
        None => (quote!(), quote!()),
    };
    // The library crates whose controls the view uses: referenced, so they are linked and their
    // classes register when the program starts.
    let link_uses = linked_crates.iter().map(|k| {
        let k = Ident::new(k, span);
        quote!(#[allow(unused_imports)] use ::#k as _;)
    });

    Ok(quote! {
        #item

        const _: () = { #(#link_uses)* };

        /// The view's text, embedded (and tracked: editing the view rebuilds the crate).
        #[doc(hidden)]
        #[allow(non_upper_case_globals)]
        const #track: &str = #text_expr;

        #default_impl

        impl #name {
            /// Links the controls of the view to this struct's fields and the view to its form —
            /// generated by `#[kubuno::view]` from the `.kbview`, the Rust twin of a Windows Forms
            /// `InitializeComponent()`. Call it once, from `new()`.
            #[allow(dead_code)]
            pub fn initialize_component(&mut self) {
                #base_form
                let members = self.__kubuno_members();
                ::kubuno::__private::initialize_view(
                    &self.__kubuno_form,
                    ::kubuno::__private::ViewSource { path: #path_expr, text: #track, display: #display },
                    &members,
                );
            }

            /// The controls of the view this struct holds (and those its `#[base]` form holds), by `x:Name`.
            #[doc(hidden)]
            #[allow(dead_code)]
            pub fn __kubuno_members(&self) -> ::std::vec::Vec<(&'static str, &::kubuno::forms::Control)> {
                #[allow(unused_mut)]
                let mut members: ::std::vec::Vec<(&'static str, &::kubuno::forms::Control)> = ::std::vec![#(#links),*];
                #base_members
                members
            }

            /// Opens the view in a window of its own and returns at once — a Windows Forms
            /// `Form.Show()` (see [`kubuno::View::show`]).
            #[allow(dead_code)]
            pub fn show(self) {
                ::kubuno::View::show(self)
            }

            /// Opens the view as a modal dialog owned by `owner`, and returns how it was closed
            /// once it is — a Windows Forms `Form.ShowDialog(owner)` (see [`kubuno::View::show_dialog`]).
            #[allow(dead_code)]
            pub fn show_dialog(&mut self, owner: &dyn ::kubuno::forms::AsForm) -> ::kubuno::DialogResult {
                ::kubuno::View::show_dialog(self, owner)
            }
        }

        impl ::kubuno::View for #name {
            const HANDLERS: &'static [&'static str] = &[#(#handler_names),*];

            fn form(&self) -> &::kubuno::Form {
                &self.__kubuno_form
            }

            fn handle_event(&mut self, handler: &str, cx: &::kubuno::__private::HandlerCx<'_>, args: &mut dyn ::kubuno::events::EventArgs) -> bool {
                #[allow(unused_variables)]
                let _ = (&cx, &args);
                match handler {
                    #(#arms)*
                    _ => false,
                }
            }
        }

        impl ::kubuno::views::binding::ViewModel for #name {
            fn get(&self, path: &str) -> ::core::option::Option<::kubuno::Value> {
                if let ::core::option::Option::Some(v) = ::kubuno::__private::form_get(&self.__kubuno_form, path) {
                    return ::core::option::Option::Some(v);
                }
                match path {
                    #(#get_binds)*
                    _ => {}
                }
                #dc_get
            }

            fn set(&mut self, path: &str, value: ::kubuno::Value) {
                let value = match ::kubuno::__private::form_set(&self.__kubuno_form, path, value) {
                    ::core::option::Option::Some(v) => v,
                    ::core::option::Option::None => return,
                };
                match path {
                    #(#set_binds)*
                    _ => {}
                }
                #dc_set
            }

            fn dispatch_event(&mut self, handler: &str, sender: &::kubuno::events::ElementRef<'_>, args: &mut dyn ::kubuno::events::EventArgs) -> bool {
                ::kubuno::__private::dispatch(self, handler, sender, args)
            }
        }

        impl ::core::ops::Deref for #name {
            type Target = ::kubuno::Form;
            fn deref(&self) -> &::kubuno::Form {
                &self.__kubuno_form
            }
        }

        impl ::core::ops::DerefMut for #name {
            fn deref_mut(&mut self) -> &mut ::kubuno::Form {
                &mut self.__kubuno_form
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn expand_xml(xml: &str, item: TokenStream2) -> String {
        expand(quote!(xml = #xml), item).to_string()
    }

    #[test]
    fn generates_a_field_per_named_control_and_a_dispatch_arm_per_handler() {
        let out = expand_xml(
            r#"<Panel OnLoad="main_view_load"><TextField x:Name="status"/><Button x:Name="hello" OnClick="hello_click" Modifiers="Public"/><Label Text="x"/></Panel>"#,
            quote!(pub struct MainView { clicks: u32 }),
        );
        assert!(out.contains("status : :: kubuno :: forms :: TextField"), "{out}");
        assert!(out.contains("pub hello : :: kubuno :: forms :: Button"), "{out}");
        assert!(out.contains("__kubuno_form : :: kubuno :: Form"), "{out}");
        assert!(out.contains("\"hello_click\" => :: kubuno :: __private :: call_handler (self , MainView :: hello_click"), "{out}");
        assert!(out.contains("\"main_view_load\""), "{out}");
        assert!(out.contains("fn initialize_component"), "{out}");
        assert!(out.contains("impl :: core :: default :: Default for MainView"), "{out}");
        assert!(out.contains("clicks : < u32 as :: core :: default :: Default > :: default ()"), "{out}");
    }

    #[test]
    fn a_derived_default_is_kept_and_bind_fields_are_read() {
        let out = expand_xml(r#"<Panel/>"#, quote!(#[derive(Default)] pub struct V { #[bind] user_name: String, #[bind("Other")] x: bool }));
        assert!(!out.contains("impl :: core :: default :: Default for V"), "{out}");
        assert!(out.contains("\"UserName\" => return"), "{out}");
        assert!(out.contains("\"Other\" => return"), "{out}");
        assert!(!out.contains("# [bind"), "helper attributes are removed: {out}");
    }

    #[test]
    fn control_fields_are_linked_not_generated() {
        let out = expand_xml(r#"<Panel><Button x:Name="ok"/></Panel>"#, quote!(pub struct V { #[control] ok: ::kubuno::forms::Custom<Round> }));
        assert!(!out.contains("# [control]"), "the helper attribute is removed: {out}");
        assert!(out.contains("(\"ok\" , :: kubuno :: forms :: AsControl :: as_control (& self . ok))"), "{out}");
        assert_eq!(out.matches("ok : :: kubuno :: forms").count(), 1, "one field `ok`, the user's: {out}");
        let out = expand_xml(r#"<Panel/>"#, quote!(pub struct V { #[control] ghost: ::kubuno::forms::Control }));
        assert!(out.contains("has no control named `ghost`"), "{out}");
    }

    #[test]
    fn folder_dependencies_are_path_and_workspace_entries() {
        let ws = std::env::temp_dir().join(format!("kubuno-view-deps-{}", std::process::id()));
        let app = ws.join("apps/chat");
        std::fs::create_dir_all(&app).expect("temp folder");
        std::fs::write(ws.join("Cargo.toml"), "[workspace]\nmembers = []\n\n[workspace.dependencies]\nchat-controls = { path = \"crates/chat-controls\" }\nserde = \"1\"\n").expect("manifest");
        let manifest = "[package]\nname = \"chat\"\n\n[dependencies]\nkubuno = { path = \"../../kubuno\" }\nchat-controls.workspace = true\nlocal = { path = \"../local\", optional = true }\nserde = \"1\"\n\n[dev-dependencies]\nother = { path = \"../other\" }\n";
        let deps = folder_dependencies(manifest, &app);
        let names: Vec<&str> = deps.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["kubuno", "local", "chat_controls"]);
        assert_eq!(deps[2].1, ws.join("crates/chat-controls"));
        let _ = std::fs::remove_dir_all(&ws);
    }

    #[test]
    fn kubuno_named_control_libraries_are_scanned_the_framework_is_not() {
        let ws = std::env::temp_dir().join(format!("kubuno-view-framework-{}", std::process::id()));
        let control = |name: &str| format!("use kubuno::prelude::*;\n\n#[derive(Component, Default)]\n#[kubuno(extends = Button)]\npub struct {name} {{\n    base: Button,\n}}\n");
        for (folder, package, class) in [
            ("kubuno-shell-controls", "kubuno-shell-controls", "WaffleMenu"),
            ("acme", "kubuno-acme-widgets", "Gauge"),
            ("kubuno", "kubuno", "FacadeOnly"),
            ("kubuno-views", "kubuno-views", "ViewsOnly"),
            ("kubuno-ui", "kubuno-ui", "UiOnly"),
        ] {
            let dir = ws.join(folder);
            std::fs::create_dir_all(dir.join("src")).expect("temp folder");
            std::fs::write(dir.join("Cargo.toml"), format!("[package]\nname = \"{package}\"\n")).expect("manifest");
            std::fs::write(dir.join("src/lib.rs"), control(class)).expect("source");
        }
        let app = ws.join("app");
        std::fs::create_dir_all(&app).expect("temp folder");
        std::fs::write(
            app.join("Cargo.toml"),
            "[package]\nname = \"app\"\n\n[dependencies]\nkubuno = { path = \"../kubuno\" }\nkubuno-views = { path = \"../kubuno-views\" }\nkubuno_ui = { path = \"../kubuno-ui\" }\nkubuno-shell-controls = { path = \"../kubuno-shell-controls\" }\nkubuno-acme-widgets = { path = \"../acme\" }\n",
        )
        .expect("manifest");
        let classes = dependency_classes_in(&app);
        let _ = std::fs::remove_dir_all(&ws);
        assert_eq!(
            classes,
            [("WaffleMenu".to_string(), "kubuno_shell_controls".to_string()), ("Gauge".to_string(), "kubuno_acme_widgets".to_string())],
            "only the control libraries are read, whatever their prefix"
        );
    }

    #[test]
    fn unknown_controls_bad_names_and_collisions_are_clear_errors() {
        let out = expand_xml("<Panel>\n  <Frobnicator x:Name=\"f\"/>\n</Panel>", quote!(pub struct V;));
        assert!(out.contains("the inline view, line 2: unknown control `<Frobnicator>` (x:Name=\\\"f\\\")"), "{out}");
        let out = expand_xml(r#"<Panel><Button x:Name="my-button"/></Panel>"#, quote!(pub struct V;));
        assert!(out.contains("is not a Rust identifier"), "{out}");
        let out = expand_xml(r#"<Panel><Button x:Name="ok"/><Label x:Name="ok"/></Panel>"#, quote!(pub struct V;));
        assert!(out.contains("two controls are named `ok`"), "{out}");
        let out = expand_xml(r#"<Panel><Button x:Name="count"/></Panel>"#, quote!(pub struct V { count: u32 }));
        assert!(out.contains("the control `count` has the name of a field of `V`"), "{out}");
        let out = expand_xml(r#"<Panel><Button></Panel>"#, quote!(pub struct V;));
        assert!(out.contains("the inline view, line 1:"), "{out}");
        let out = expand_xml(r#"<Panel/>"#, quote!(pub struct V<T>(T);));
        assert!(out.contains("a view cannot be generic"), "{out}");
        let out = expand(quote!(), quote!(pub struct V;)).to_string();
        assert!(out.contains("the view is missing"), "{out}");
        let out = expand_xml(r#"<Panel/>"#, quote!(fn not_a_struct() {}));
        assert!(out.contains("goes on a struct"), "{out}");
    }
}
