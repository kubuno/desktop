//! `settings!` — the typed settings class of a `.kbsettings` file (vskubuno `docs/STORAGE-COMPONENTS.md` §5.3),
//! Windows Forms' generated `Properties.Settings`:
//!
//! ```ignore
//! kubuno::settings!("settings.kbsettings");     // pub struct Settings
//!
//! let theme: String = Settings::theme();         // the user's value, else the machine's, else the default
//! Settings::set_theme("Dark");                   // changed and saved (a failure is logged, never the value)
//! let channel = Settings::update_channel();      // an Application setting: no setter
//! let _s = Settings::on_changed(|c| println!("{} changed", c.name));
//! Settings::store().reload();                     // the shared `kubuno_app_storage::Settings` behind it
//! ```
//!
//! The file is read at compile time (an invalid file is a compile error at the path literal) and embedded for
//! the compiler to track it. The class registers its schema before `main` (and on first use), so a
//! `<Settings Schema="settings">` component of a view binds to the same values.

use std::path::Path;

use kubuno_resources_model::names;
use kubuno_resources_model::settings::{SettingEntry, SettingsFile};
use kubuno_resources_model::Severity;
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token, Visibility};

/// `krate = <path>; [vis] [Name,] "file.kbsettings"`.
struct Input {
    krate: TokenStream2,
    vis: Visibility,
    name: Option<Ident>,
    path: LitStr,
}

impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut krate = quote!(::kubuno_app_storage);
        if input.peek(Ident) && input.peek2(Token![=]) {
            let key: Ident = input.parse()?;
            if key != "krate" {
                return Err(syn::Error::new(key.span(), "expected a `.kbsettings` path: settings!(\"settings.kbsettings\")"));
            }
            input.parse::<Token![=]>()?;
            let mut tokens = TokenStream2::new();
            while !input.peek(Token![;]) {
                if input.is_empty() {
                    return Err(input.error("expected `;` after `krate = <path>`"));
                }
                let tt: proc_macro2::TokenTree = input.parse()?;
                tokens.extend([tt]);
            }
            input.parse::<Token![;]>()?;
            krate = tokens;
        }
        let vis: Visibility = match input.parse()? {
            Visibility::Inherited => Visibility::Public(Token![pub](Span::call_site())),
            v => v,
        };
        let name = if input.peek(Ident) {
            let n: Ident = input.parse()?;
            input.parse::<Token![,]>()?;
            Some(n)
        } else {
            None
        };
        let path: LitStr = input.parse().map_err(|e| syn::Error::new(e.span(), "expected a `.kbsettings` path: settings!(\"settings.kbsettings\")"))?;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("settings! takes a `.kbsettings` path, optionally preceded by a visibility and a type name: settings!(pub(crate) AppSettings, \"app.kbsettings\")"));
        }
        Ok(Input { krate, vis, name, path })
    }
}

/// The generated members that a setting's accessor must not shadow.
const RESERVED: &[&str] = &["app", "schema", "register", "store", "save", "reload", "reset", "reset_all", "on_changed", "refresh_if_changed", "get", "set", "names", "location"];

pub(crate) fn settings(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as Input);
    match expand(&input) {
        Ok(ts) => ts.into(),
        Err(message) => syn::Error::new(input.path.span(), message).to_compile_error().into(),
    }
}

fn expand(input: &Input) -> Result<TokenStream2, String> {
    if !input.path.value().to_ascii_lowercase().ends_with(".kbsettings") {
        return Err(format!("`{}` is not a `.kbsettings` file", input.path.value()));
    }
    let path = crate::resolve(&input.path).map_err(|e| e.replace("resource file", "settings file").replace("resources!", "settings!"))?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;
    let file_name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let (file, diags) = SettingsFile::read(&text);
    if let Some(d) = diags.iter().find(|d| d.severity == Severity::Error) {
        let line = text[..d.range.start.min(text.len())].matches('\n').count() + 1;
        return Err(format!("`{file_name}`, line {line}: {}", d.message));
    }
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_else(|| "settings".to_string());
    let set_ok = !stem.is_empty() && stem.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-' | '_')) && stem.starts_with(|c: char| c.is_ascii_alphanumeric());
    if !set_ok {
        return Err(format!("`{file_name}`: the file name must be a valid set name (letters, digits, `.`, `-`, `_`)"));
    }
    let type_name = input.name.clone().unwrap_or_else(|| Ident::new(&names::type_name(&stem), Span::call_site()));
    generate(&input.krate, &input.vis, &type_name, &stem, &path, &file)
}

fn value_tokens(krate: &TokenStream2, e: &SettingEntry) -> TokenStream2 {
    match e.ty.as_str() {
        "Bool" => {
            let b = e.default == "true";
            quote!(#krate::SettingValue::Bool(#b))
        }
        "Int" => {
            let i: i64 = e.default.trim().parse().unwrap_or(0);
            quote!(#krate::SettingValue::Int(#i))
        }
        "Float" => {
            let f: f64 = e.default.trim().parse().unwrap_or(0.0);
            quote!(#krate::SettingValue::Float(#f))
        }
        "StringList" => {
            let items = e.default_items();
            quote!(#krate::SettingValue::StringList(::std::vec![#(::std::string::String::from(#items)),*]))
        }
        _ => {
            let s = &e.default;
            quote!(#krate::SettingValue::String(::std::string::String::from(#s)))
        }
    }
}

fn rust_type(ty: &str) -> TokenStream2 {
    match ty {
        "Bool" => quote!(bool),
        "Int" => quote!(i64),
        "Float" => quote!(f64),
        "StringList" => quote!(::std::vec::Vec<::std::string::String>),
        _ => quote!(::std::string::String),
    }
}

fn generate(krate: &TokenStream2, vis: &Visibility, type_name: &Ident, set: &str, path: &Path, file: &SettingsFile) -> Result<TokenStream2, String> {
    let abs = path.to_string_lossy().into_owned();
    let file_name = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let version = file.version;
    let account_scoped = file.account_scoped;
    let store_body = if account_scoped {
        quote! {
            Self::register();
            #krate::Settings::shared_or_memory(&Self::app(), &Self::schema(), #krate::backend::BackendKind::Auto)
        }
    } else {
        quote! {
            static STORE: ::std::sync::OnceLock<#krate::Settings> = ::std::sync::OnceLock::new();
            STORE
                .get_or_init(|| {
                    Self::register();
                    #krate::Settings::shared_or_memory(&Self::app(), &Self::schema(), #krate::backend::BackendKind::Auto)
                })
                .clone()
        }
    };
    let app = match &file.app {
        Some(a) => quote!(#a),
        None => quote!(::core::env!("CARGO_PKG_NAME")),
    };
    let mut accessors = Vec::new();
    let mut defs = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for e in &file.entries {
        let snake = names::rust_name(&e.name);
        if RESERVED.contains(&snake.as_str()) {
            return Err(format!("`{file_name}`: the setting `{}` would generate `{snake}()`, a member every settings class has: rename the setting", e.name));
        }
        if seen.contains(&snake) {
            return Err(format!("`{file_name}`: two settings generate the accessor `{snake}()`: rename one of them"));
        }
        seen.push(snake.clone());
        let name = &e.name;
        let ty_ident = format_ident!("{}", e.ty);
        let scope_ident = format_ident!("{}", e.scope);
        let roaming = e.roaming;
        let default = value_tokens(krate, e);
        let description = &e.description;
        let previous = &e.previous_names;
        let values = &e.values;
        defs.push(quote! {
            #krate::SettingDef {
                name: ::std::string::String::from(#name),
                ty: #krate::SettingType::#ty_ident,
                scope: #krate::SettingScope::#scope_ident,
                roaming: #roaming,
                default: #default,
                description: ::std::string::String::from(#description),
                previous_names: ::std::vec![#(::std::string::String::from(#previous)),*],
                values: ::std::vec![#(::std::string::String::from(#values)),*],
            }
        });
        let rty = rust_type(&e.ty);
        let getter = format_ident!("{}", snake);
        let where_ = if e.is_application() { "application, read-only".to_string() } else if e.roaming { "user, roaming".to_string() } else { "user, this machine only".to_string() };
        let shown_default = if e.ty == "StringList" { format!("{:?}", e.default_items()) } else { format!("`{}`", e.default) };
        let mut doc = format!("**{}** `{}` ({where_}) — default {shown_default}.", e.ty, e.name);
        if !e.values.is_empty() {
            doc.push_str(&format!(" One of: {}.", e.values.join(", ")));
        }
        if !e.description.is_empty() {
            doc.push_str(&format!("\n\n{}", e.description));
        }
        accessors.push(quote! {
            #[doc = #doc]
            pub fn #getter() -> #rty {
                Self::store().get_as::<#rty>(#name).unwrap_or_else(|| <#rty as #krate::FromSetting>::from_setting(&#default).unwrap_or_default())
            }
        });
        if !e.is_application() {
            let setter = format_ident!("set_{}", snake);
            let set_doc = format!("Changes `{}` and saves it (a failure is logged with the setting's name, never its value; use [`Self::store`] to handle it).", e.name);
            accessors.push(quote! {
                #[doc = #set_doc]
                pub fn #setter(value: impl ::core::convert::Into<#rty>) {
                    #krate::settings::set_and_save(&Self::store(), #name, #krate::SettingValue::from(value.into()));
                }
            });
        }
    }
    let names_list: Vec<&str> = file.entries.iter().map(|e| e.name.as_str()).collect();
    let type_doc = format!("The settings of `{file_name}` (set `{set}`, version {version}), generated by `settings!`: one accessor per setting, and [`{type_name}::store`] for the shared `Settings` behind them.");
    Ok(quote! {
        #[doc = #type_doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        #vis struct #type_name;

        #[allow(dead_code)]
        impl #type_name {
            /// The set's name (the file's stem): what `<Settings Schema="…">` names.
            pub const SET: &'static str = #set;
            /// The schema version (`Version`).
            pub const VERSION: u32 = #version;
            /// The declared settings, in file order.
            pub const NAMES: &'static [&'static str] = &[#(#names_list),*];

            /// The app id the values are stored under (`App`, else the package name).
            pub fn app() -> #krate::AppId {
                #krate::AppId::from_name(#app).unwrap_or_else(#krate::default_app_id)
            }

            /// The declared settings.
            pub fn schema() -> #krate::SettingsSchema {
                const _: &str = ::core::include_str!(#abs);
                #krate::SettingsSchema {
                    set: ::std::string::String::from(#set),
                    version: #version,
                    open: false,
                    open_local: false,
                    account_scoped: #account_scoped,
                    defs: ::std::vec![#(#defs),*],
                }
            }

            /// Registers the schema for the `<Settings>` components of views (done before `main` and on first use).
            pub fn register() {
                static ONCE: ::std::sync::Once = ::std::sync::Once::new();
                ONCE.call_once(|| #krate::settings::register_schema(&Self::app(), Self::schema()));
            }

            /// The shared settings behind the accessors (files of the platform; memory if they cannot be opened). For
            /// an account-scoped file, those of the current account (`set_current_account`), looked up at each call.
            pub fn store() -> #krate::Settings {
                #store_body
            }

            /// Stores the pending changes (the setters already save).
            pub fn save() -> ::core::result::Result<(), #krate::StorageError> {
                Self::store().save()
            }

            /// Reads the values again, dropping unsaved changes.
            pub fn reload() {
                Self::store().reload()
            }

            /// Forgets every user value (back to the machine's values and the defaults), then saves.
            pub fn reset_all() -> ::core::result::Result<(), #krate::StorageError> {
                let s = Self::store();
                s.reset_all()?;
                s.save()
            }

            /// Reloads when another instance or an administrator changed the stored values.
            pub fn refresh_if_changed() -> bool {
                Self::store().refresh_if_changed()
            }

            /// Calls `f` after each change (keep the subscription alive).
            pub fn on_changed(f: impl Fn(&#krate::SettingChange) + ::core::marker::Send + ::core::marker::Sync + 'static) -> #krate::Subscription {
                Self::store().subscribe(f)
            }

            #(#accessors)*
        }

        // Registered before `main`, so the views' `<Settings>` components find the schema at once.
        #[cfg(windows)]
        const _: () = {
            #[used]
            #[unsafe(link_section = ".CRT$XCU")]
            static __KUBUNO_SETTINGS_INIT: extern "C" fn() = __init;
            extern "C" fn __init() {
                #type_name::register();
            }
        };
    })
}
