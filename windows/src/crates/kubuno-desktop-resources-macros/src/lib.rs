//! # `kubuno-desktop-resources-macros` — `resources!`
//!
//! The procedural macro behind `kubuno_desktop_resources::resources!` / `kubuno_desktop::resources!`
//! (`vskubuno/docs/RESOURCES.md`). That `macro_rules!` forwards `$crate`, so the expansion reaches
//! `kubuno_desktop_resources` whatever the application depends on.
//!
//! `resources!("resources.kbres")` reads the neutral file (an invalid file is a compile error at the
//! path literal), finds its satellites beside it (`resources.fr.kbres`…), and generates:
//!
//! ```ignore
//! /// Resources of `resources.kbres` (cultures: fr, de-DE).
//! pub struct Resources;
//! impl Resources {
//!     /// **String** `welcome_text` — "Welcome!" …
//!     pub fn welcome_text() -> &'static str;
//!     pub fn logo() -> kubuno_desktop_resources::Image;      // Image
//!     pub fn app() -> kubuno_desktop_resources::Icon;        // Icon
//!     pub fn ding() -> kubuno_desktop_resources::Audio;      // Audio
//!     pub fn license() -> &'static str;              // File with Text="true" (else &'static [u8])
//!     pub fn accent() -> kubuno_desktop_resources::Color;    // Color
//!     pub fn heading() -> kubuno_desktop_resources::FontSpec;// Font
//!     pub fn culture() -> String;  pub fn set_culture(culture: &str);
//!     pub fn register();  pub fn set() -> &'static kubuno_desktop_resources::StaticSet;
//!     pub const NAMES: &'static [&'static str];
//! }
//! ```
//!
//! Every file is embedded with `include_str!`/`include_bytes!` (absolute paths), so the compiler
//! rebuilds when one changes and nothing is read at run time. A satellite added later is seen at
//! the next expansion (touch the neutral file — the resource editor saves it).

use std::path::{Path, PathBuf};

use kubuno_desktop_resources_model::{culture, names, set, Kind, ResourceFile, Value};
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Ident, LitStr, Token, Visibility};

/// `krate = <path>; [vis] [Name,] "file.kbres"`.
struct Input {
    krate: TokenStream2,
    vis: Visibility,
    name: Option<Ident>,
    path: LitStr,
}

impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut krate = quote!(::kubuno_desktop_resources);
        if input.peek(Ident) && input.peek2(Token![=]) {
            let key: Ident = input.parse()?;
            if key != "krate" {
                return Err(syn::Error::new(key.span(), "expected a `.kbres` path: resources!(\"resources.kbres\")"));
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
        // No visibility written: `pub` (the generated class is meant to be used across the crate).
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
        let path: LitStr = input.parse().map_err(|e| syn::Error::new(e.span(), "expected a `.kbres` path: resources!(\"resources.kbres\")"))?;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("resources! takes a `.kbres` path, optionally preceded by a visibility and a type name: resources!(pub(crate) Strings, \"strings.kbres\")"));
        }
        Ok(Input { krate, vis, name, path })
    }
}

/// The typed settings class of a `.kbsettings` file (`kubuno_desktop::settings!`, vskubuno `docs/STORAGE-COMPONENTS.md`).
mod settings;

/// See the `settings` module; documented on `kubuno_desktop::settings!`.
#[proc_macro]
pub fn settings(input: TokenStream) -> TokenStream {
    settings::settings(input)
}

/// See the crate doc; documented on `kubuno_desktop_resources::resources!`.
#[proc_macro]
pub fn resources(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as Input);
    match expand(&input) {
        Ok(ts) => ts.into(),
        Err(message) => syn::Error::new(input.path.span(), message).to_compile_error().into(),
    }
}

/// What the generator needs, read from disk.
struct Loaded {
    neutral_path: PathBuf,
    neutral_text: String,
    file: ResourceFile,
    /// `(culture, path, parsed)`.
    satellites: Vec<(String, PathBuf, ResourceFile)>,
}

fn expand(input: &Input) -> Result<TokenStream2, String> {
    let path = resolve(&input.path)?;
    let loaded = load(&path)?;
    let stem = set::discover(&loaded.neutral_path).map(|s| s.name).unwrap_or_else(|| "resources".to_string());
    let type_name = input.name.clone().unwrap_or_else(|| Ident::new(&names::type_name(&stem), Span::call_site()));
    generate(&input.krate, &input.vis, &type_name, &stem, &loaded)
}

fn load(path: &Path) -> Result<Loaded, String> {
    let files = set::discover(path).ok_or_else(|| format!("`{}` is not a `.kbres` file", path.display()))?;
    let display = |p: &Path| p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let neutral_text = std::fs::read_to_string(&files.neutral).map_err(|e| format!("cannot read `{}`: {e}", files.neutral.display()))?;
    let file = ResourceFile::parse(&neutral_text).map_err(|d| format!("`{}`: {} (at byte {})", display(&files.neutral), d.message, d.range.start))?;
    let mut satellites = Vec::new();
    for s in &files.satellites {
        let text = std::fs::read_to_string(&s.path).map_err(|e| format!("cannot read `{}`: {e}", s.path.display()))?;
        let parsed = ResourceFile::parse(&text).map_err(|d| format!("`{}`: {} (at byte {})", display(&s.path), d.message, d.range.start))?;
        for d in set::check_satellite(&file, &s.culture, &parsed) {
            if d.diagnostic.severity == kubuno_desktop_resources_model::Severity::Error {
                return Err(format!("`{}`: {}", display(&s.path), d.diagnostic.message));
            }
        }
        satellites.push((s.culture.clone(), s.path.clone(), parsed));
    }
    Ok(Loaded { neutral_path: files.neutral, neutral_text, file, satellites })
}

fn generate(krate: &TokenStream2, vis: &Visibility, type_name: &Ident, stem: &str, loaded: &Loaded) -> Result<TokenStream2, String> {
    let dir = loaded.neutral_path.parent().map(Path::to_path_buf).unwrap_or_default();
    let neutral_abs = loaded.neutral_path.to_string_lossy().into_owned();
    let file_name = loaded.neutral_path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();

    // Linked files of every culture, once each, checked to exist.
    let mut linked: Vec<String> = Vec::new();
    for e in loaded.file.entries.iter().chain(loaded.satellites.iter().flat_map(|(_, _, f)| f.entries.iter())) {
        if let Value::Linked { path } = &e.value {
            if !linked.contains(path) {
                let full = dir.join(path.replace('/', std::path::MAIN_SEPARATOR_STR));
                if !full.is_file() {
                    return Err(format!("`{file_name}`: the file of `{}` does not exist: `{path}` (relative to the .kbres file)", e.name));
                }
                linked.push(path.clone());
            }
        }
    }
    let files = linked.iter().map(|p| {
        let full = dir.join(p.replace('/', std::path::MAIN_SEPARATOR_STR)).to_string_lossy().into_owned();
        quote! { (#p, include_bytes!(#full)) }
    });
    let satellites = loaded.satellites.iter().map(|(c, p, _)| {
        let full = p.to_string_lossy().into_owned();
        quote! { (#c, include_str!(#full)) }
    });

    // One accessor per entry; generated names must not collide.
    let mut seen: Vec<(String, String)> = Vec::new();
    let mut accessors = Vec::new();
    let mut entry_names = Vec::new();
    for e in &loaded.file.entries {
        let rust = names::rust_name(&e.name);
        if let Some((_, other)) = seen.iter().find(|(r, _)| *r == rust) {
            return Err(format!("`{file_name}`: `{}` and `{other}` both generate the accessor `{rust}()`: rename one", e.name));
        }
        seen.push((rust.clone(), e.name.clone()));
        let fn_name = format_ident!("{}", rust);
        let key = e.name.as_str();
        entry_names.push(key.to_string());
        let doc = entry_doc(e, &loaded.satellites);
        let body = match e.kind {
            Kind::String => quote! { pub fn #fn_name() -> &'static str { Self::set().text(#key) } },
            Kind::Color => quote! { pub fn #fn_name() -> #krate::Color { #krate::Color::parse(Self::set().text(#key)) } },
            Kind::Font => quote! { pub fn #fn_name() -> #krate::FontSpec { #krate::FontSpec(Self::set().text(#key)) } },
            Kind::Image => quote! { pub fn #fn_name() -> #krate::Image { let (b, f) = Self::set().bytes(#key); #krate::Image::new(#stem, #key, b, f) } },
            Kind::Icon => quote! { pub fn #fn_name() -> #krate::Icon { let (b, f) = Self::set().bytes(#key); #krate::Icon::new(#stem, #key, b, f) } },
            Kind::Audio => quote! { pub fn #fn_name() -> #krate::Audio { let (b, f) = Self::set().bytes(#key); #krate::Audio::new(b, f) } },
            Kind::File if e.text_file => quote! { pub fn #fn_name() -> &'static str { Self::set().text(#key) } },
            Kind::File => quote! { pub fn #fn_name() -> &'static [u8] { Self::set().bytes(#key).0 } },
        };
        accessors.push(quote! { #[doc = #doc] #body });
    }
    let cultures: Vec<&str> = loaded.satellites.iter().map(|(c, _, _)| c.as_str()).collect();
    let type_doc = format!(
        "Resources of `{file_name}`{} — generated by `resources!`: one accessor per entry, in the current UI culture ([`{}::culture`]).",
        if cultures.is_empty() { String::new() } else { format!(" (cultures: {})", cultures.join(", ")) },
        type_name
    );
    let _ = &loaded.neutral_text;
    Ok(quote! {
        #[doc = #type_doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
        #vis struct #type_name;

        #[allow(dead_code)]
        impl #type_name {
            /// The names of the resources, in file order.
            pub const NAMES: &'static [&'static str] = &[#(#entry_names),*];

            /// The embedded set behind the accessors (registered for `{Res …}` lookups on first use).
            pub fn set() -> &'static #krate::StaticSet {
                static EMBEDDED: #krate::EmbeddedSet = #krate::EmbeddedSet {
                    name: #stem,
                    neutral: include_str!(#neutral_abs),
                    satellites: &[#(#satellites),*],
                    files: &[#(#files),*],
                };
                static SET: #krate::StaticSet = #krate::StaticSet::new(&EMBEDDED);
                static REGISTERED: ::std::sync::Once = ::std::sync::Once::new();
                REGISTERED.call_once(|| #krate::register_static(&SET));
                &SET
            }

            /// Registers these resources for `{Res …}` lookups in views (done when the program
            /// starts, and on first use of an accessor).
            pub fn register() {
                let _ = Self::set();
            }

            /// The current UI culture of the process.
            pub fn culture() -> ::std::string::String {
                #krate::culture()
            }

            /// Sets the UI culture of the process (views bound with `{Res …}` repaint).
            pub fn set_culture(culture: &str) {
                #krate::set_culture(culture)
            }

            /// The value of `name` (any kind) in the current culture, looked up by name at run time.
            pub fn get(name: &str) -> ::core::option::Option<#krate::ResolvedValue> {
                #krate::Source::resolve(Self::set(), name, &#krate::culture())
            }

            #(#accessors)*
        }

        // Registered before `main`, so `{Res …}` finds these resources even before an accessor runs.
        #[cfg(windows)]
        const _: () = {
            #[used]
            #[unsafe(link_section = ".CRT$XCU")]
            static __KUBUNO_RESOURCES_INIT: extern "C" fn() = __init;
            extern "C" fn __init() {
                #type_name::register();
            }
        };
    })
}

/// The doc comment of an accessor: kind, name, neutral value (truncated), translations, comment.
fn entry_doc(e: &kubuno_desktop_resources_model::Entry, satellites: &[(String, PathBuf, ResourceFile)]) -> String {
    let mut doc = format!("**{}** `{}`", e.kind.element(), e.name);
    match &e.value {
        Value::Text(t) => doc.push_str(&format!(" — {}", quote_short(t))),
        Value::Linked { path } => doc.push_str(&format!(" — file `{path}`")),
        Value::Embedded { format, bytes } => doc.push_str(&format!(" — embedded {format}, {} bytes", bytes.len())),
    }
    if let Some(c) = &e.comment {
        doc.push_str(&format!("\n\n{c}"));
    }
    let translated: Vec<String> = satellites
        .iter()
        .filter_map(|(c, _, f)| {
            f.get(&e.name).map(|s| match &s.value {
                Value::Text(t) => format!("- `{c}`: {}", quote_short(t)),
                Value::Linked { path } => format!("- `{c}`: file `{path}`"),
                Value::Embedded { format, .. } => format!("- `{c}`: embedded {format}"),
            })
        })
        .collect();
    if !translated.is_empty() {
        doc.push_str("\n\nTranslations:\n");
        doc.push_str(&translated.join("\n"));
    }
    doc
}

fn quote_short(t: &str) -> String {
    let one_line = t.replace(['\r', '\n'], " ");
    let short: String = one_line.chars().take(80).collect();
    if short.len() < one_line.len() {
        format!("\"{short}…\"")
    } else {
        format!("\"{short}\"")
    }
}

/// Resolves the `.kbres` path like `include_str!`: relative to the file holding the macro call,
/// else (tools that do not know that file: rust-analyzer's proc-macro server) the package's `src`
/// folder and root, else the one file under `src` whose path ends with the given one.
fn resolve(path: &LitStr) -> Result<PathBuf, String> {
    let rel = PathBuf::from(path.value());
    if rel.is_absolute() {
        return if rel.is_file() { Ok(rel) } else { Err(format!("cannot find the resource file `{}`", rel.display())) };
    }
    let manifest = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from);
    let mut tried = Vec::new();
    if let Some(dir) = path.span().unwrap().local_file().and_then(|f| f.parent().map(Path::to_path_buf)) {
        if dir.is_relative() {
            if let Ok(cwd) = std::env::current_dir() {
                tried.push(cwd.join(&dir).join(&rel));
            }
            if let Some(m) = &manifest {
                tried.push(m.join(&dir).join(&rel));
            }
        } else {
            tried.push(dir.join(&rel));
        }
    }
    if let Some(m) = &manifest {
        tried.push(m.join("src").join(&rel));
        tried.push(m.join(&rel));
    }
    if let Some(found) = tried.iter().find(|p| p.is_file()) {
        return Ok(normalize(found));
    }
    if let Some(m) = &manifest {
        if let Some(found) = unique_under(&m.join("src"), &rel)? {
            return Ok(normalize(&found));
        }
    }
    Err(format!("cannot find the resource file `{}` (looked relative to the file holding resources!, then in the package's `src` folder and root)", path.value()))
}

fn unique_under(root: &Path, rel: &Path) -> Result<Option<PathBuf>, String> {
    let wanted: Vec<_> = rel.components().filter(|c| !matches!(c, std::path::Component::CurDir)).collect();
    if wanted.is_empty() {
        return Ok(None);
    }
    let mut matches = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(_) => {
                    let c: Vec<_> = p.components().collect();
                    if c.len() >= wanted.len() && c[c.len() - wanted.len()..] == wanted[..] {
                        matches.push(p);
                    }
                }
                Err(_) => {}
            }
        }
    }
    match matches.len() {
        0 => Ok(None),
        1 => Ok(matches.pop()),
        _ => {
            matches.sort();
            let list: Vec<String> = matches.iter().map(|p| p.display().to_string()).collect();
            Err(format!("`{}` matches several files under `src` ({}): give its path relative to the file holding resources!", rel.display(), list.join(", ")))
        }
    }
}

/// `a/./b/../c` → `a/c`, lexically (no `canonicalize`: on a mapped network drive it gives a
/// `\\?\UNC\…` path).
fn normalize(p: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The culture names of a set, for tests and tools.
#[allow(dead_code)]
fn cultures_of(loaded: &Loaded) -> Vec<String> {
    loaded.satellites.iter().map(|(c, _, _)| culture::canonical(c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str, files: &[(&str, &[u8])]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kbres-macro-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (f, content) in files {
            let p = dir.join(f);
            std::fs::create_dir_all(p.parent().expect("parent")).expect("dir");
            std::fs::write(p, content).expect("write");
        }
        dir
    }

    fn gen(dir: &Path) -> Result<String, String> {
        let loaded = load(&dir.join("resources.kbres"))?;
        let ts = generate(&quote!(::kubuno_desktop_resources), &Visibility::Public(Token![pub](Span::call_site())), &format_ident!("Resources"), "resources", &loaded)?;
        Ok(ts.to_string())
    }

    #[test]
    fn generates_typed_accessors_and_embeds_every_file() {
        let dir = fixture(
            "ok",
            &[
                ("resources.kbres", br##"<Resources><String Name="WelcomeText" Comment="Greeting">Welcome</String><Image Name="logo" File="img/logo.png"/><Icon Name="app" File="app.ico"/><Audio Name="ding" File="ding.wav"/><File Name="license" File="LICENSE.txt" Text="true"/><File Name="blob" Format="bin">AAEC</File><Color Name="accent" Value="#3366FF"/><Font Name="heading" Value="Segoe UI, 14pt"/></Resources>"##),
                ("resources.fr.kbres", br#"<Resources><String Name="WelcomeText">Bienvenue</String><Image Name="logo" File="img/logo.fr.png"/></Resources>"#),
                ("img/logo.png", b"png"),
                ("img/logo.fr.png", b"png-fr"),
                ("app.ico", b"ico"),
                ("ding.wav", b"wav"),
                ("LICENSE.txt", b"text"),
            ],
        );
        let out = gen(&dir).expect("generates");
        for want in [
            "pub fn welcome_text () -> & 'static str",
            "pub fn logo () -> :: kubuno_desktop_resources :: Image",
            "pub fn app () -> :: kubuno_desktop_resources :: Icon",
            "pub fn ding () -> :: kubuno_desktop_resources :: Audio",
            "pub fn license () -> & 'static str",
            "pub fn blob () -> & 'static [u8]",
            "pub fn accent () -> :: kubuno_desktop_resources :: Color",
            "pub fn heading () -> :: kubuno_desktop_resources :: FontSpec",
            "(\"fr\" , include_str !",
            "(\"img/logo.fr.png\" , include_bytes !",
            ".CRT$XCU",
            "Bienvenue",
        ] {
            assert!(out.contains(want), "missing `{want}` in:\n{out}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn reports_invalid_files_missing_links_and_collisions() {
        let bad = fixture("bad", &[("resources.kbres", b"<Resources><String Name='a'>x</String><String Name='a'>y</String></Resources>")]);
        assert!(gen(&bad).unwrap_err().contains("duplicate resource `a`"));
        let missing = fixture("missing", &[("resources.kbres", b"<Resources><Image Name='logo' File='nope.png'/></Resources>")]);
        assert!(gen(&missing).unwrap_err().contains("does not exist: `nope.png`"));
        let collide = fixture("collide", &[("resources.kbres", b"<Resources><String Name='OkText'>x</String><String Name='ok_text'>y</String></Resources>")]);
        assert!(gen(&collide).unwrap_err().contains("both generate the accessor `ok_text()`"));
        let kind = fixture("kind", &[("resources.kbres", b"<Resources><String Name='a'>x</String></Resources>"), ("resources.fr.kbres", b"<Resources><Color Name='a' Value='#fff'/></Resources>")]);
        assert!(gen(&kind).unwrap_err().contains("is a String in the neutral file"));
        for d in [bad, missing, collide, kind] {
            let _ = std::fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn finds_the_file_under_src_without_the_calling_file() {
        let root = fixture("unique", &[("src/res/resources.kbres", b"x"), ("src/a/strings.kbres", b"x"), ("src/b/strings.kbres", b"x")]);
        let src = root.join("src");
        assert_eq!(unique_under(&src, Path::new("resources.kbres")), Ok(Some(src.join("res").join("resources.kbres"))));
        assert!(unique_under(&src, Path::new("strings.kbres")).unwrap_err().contains("several files"));
        assert_eq!(unique_under(&src, Path::new("b/strings.kbres")), Ok(Some(src.join("b").join("strings.kbres"))));
        let _ = std::fs::remove_dir_all(&root);
    }
}
