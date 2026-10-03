//! # `kubuno-data-macros` — `data_source!`
//!
//! The procedural macro behind `kubuno_data::data_source!` (`vskubuno/docs/DATA.md`, DATA-4). Use it
//! through `kubuno_data::data_source!("shop.kbdata")` (or `kubuno::data::data_source!`): that
//! `macro_rules!` forwards `$crate`, so the expansion reaches `kubuno_data` (and `sqlx` through
//! `kubuno_data::sqlx`) whatever the application depends on.
//!
//! The `.kbdata` path is resolved like `include_str!` (relative to the file holding the macro), then
//! to the package's `src` folder and root. Every statement is expanded by sqlx's own query macro code
//! (`sqlx-macros-core`): in offline mode (no `DATABASE_URL`, or `SQLX_OFFLINE=true`) it is checked
//! against the committed `.sqlx/query-<hash>.json` files; online (`DATABASE_URL` set,
//! `SQLX_OFFLINE` not true) against the database, and written to `SQLX_OFFLINE_DIR` when that
//! variable names a folder (how the cache is regenerated). See `kubuno_data::data_source!` for the
//! generated items.

use std::path::{Path, PathBuf};

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::parse::{Parse, ParseStream};
use syn::{LitStr, Token};

mod expand;
mod rewrite;

use expand::{Ctx, SqlxCall};

/// `krate = <path>; "file.kbdata"` (through `kubuno_data::data_source!`) or `"file.kbdata"`.
struct Input {
    krate: TokenStream2,
    path: LitStr,
}

impl Parse for Input {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut krate = quote!(::kubuno_data);
        if input.peek(syn::Ident) {
            let key: syn::Ident = input.parse()?;
            if key != "krate" {
                return Err(syn::Error::new(key.span(), "expected a `.kbdata` path: data_source!(\"shop.kbdata\")"));
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
        let path: LitStr = input.parse().map_err(|e| syn::Error::new(e.span(), "expected a `.kbdata` path: data_source!(\"shop.kbdata\")"))?;
        if input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
        }
        if !input.is_empty() {
            return Err(input.error("data_source! takes one argument: the `.kbdata` path"));
        }
        Ok(Input { krate, path })
    }
}

/// See the crate doc; documented on `kubuno_data::data_source!`.
#[proc_macro]
pub fn data_source(input: TokenStream) -> TokenStream {
    let input = syn::parse_macro_input!(input as Input);
    match data_source_impl(&input) {
        Ok(ts) => ts.into(),
        Err(message) => syn::Error::new(input.path.span(), message).to_compile_error().into(),
    }
}

fn data_source_impl(input: &Input) -> Result<TokenStream2, String> {
    let file = resolve(&input.path)?;
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from);
    let display = display_path(&file, manifest_dir.as_deref(), &input.path.value());
    let text = std::fs::read_to_string(&file).map_err(|e| format!("cannot read `{display}`: {e}"))?;
    let source = kubuno_data_model::DataSource::parse(&text).map_err(|e| format!("`{display}` is not a valid .kbdata file: {e}"))?;
    let plan = kubuno_data_model::plan(&source).map_err(|e| format!("`{display}`: {e}"))?;
    check_provider(&plan, &display)?;

    let ctx = Ctx { krate: input.krate.clone(), display: display.clone() };
    let krate = input.krate.clone();
    let mut expander = |call: &SqlxCall<'_>| expand_sqlx(call, &krate);
    let items = expand::generate(&plan, &ctx, &mut expander).map_err(|e| format!("`{display}`: {e}"))?;

    // Rebuild when the `.kbdata` or a cache file it uses changes, or when the cache mode changes
    // (`option_env!` records the variables in the dep-info; DATABASE_URL is deliberately not
    // recorded: it may hold a password, and the dep-info files keep the values).
    let file_str = file.to_string_lossy().into_owned();
    let mut cache_dirs: Vec<PathBuf> = Vec::new();
    if let Some(m) = &manifest_dir {
        cache_dirs.push(m.join(".sqlx"));
        if let Some(ws) = workspace_root(m) {
            if ws != *m {
                cache_dirs.push(ws.join(".sqlx"));
            }
        }
    }
    let cache_files: Vec<String> = if plan.provider == kubuno_data_model::ProviderName::Sqlserver {
        Vec::new()
    } else {
        plan.statements()
            .iter()
            .filter_map(|(_, st)| cache_dirs.iter().map(|d| d.join(st.cache_file_name())).find(|p| p.is_file()))
            .map(|p| p.to_string_lossy().into_owned())
            .collect()
    };
    // The migrations too: the stale-cache warning compares them with the cache (an edited migration
    // re-expands the macro; a new file cannot be tracked by rustc — Visual Studio's "Add migration"
    // touches the crate's `.kbdata` files for that).
    let migration_files: Vec<String> = manifest_dir
        .as_deref()
        .and_then(|m| std::fs::read_dir(m.join("migrations")).ok())
        .map(|entries| {
            let mut files: Vec<String> =
                entries.flatten().map(|e| e.path()).filter(|p| p.is_file() && p.extension().is_some_and(|x| x == "sql")).map(|p| p.to_string_lossy().into_owned()).collect();
            files.sort();
            files
        })
        .unwrap_or_default();
    // The use carries the path literal's span, so the warning points at the application's
    // `data_source!` call rather than at kubuno-data's `macro_rules!` wrapper.
    let path_span = input.path.span();
    let warning = stale_warning(&plan, manifest_dir.as_deref(), &cache_dirs, &display).map(|note| {
        let use_it = quote_spanned! {path_span=> let _ = kubuno_data_stale_sqlx_cache; };
        quote! {
            const _: () = {
                #[deprecated(note = #note)]
                #[allow(non_camel_case_types)]
                struct kubuno_data_stale_sqlx_cache;
                #use_it
            };
        }
    });
    Ok(quote! {
        #items

        const _: () = {
            let _ = ::core::include_bytes!(#file_str);
            #(let _ = ::core::include_bytes!(#cache_files);)*
            #(let _ = ::core::include_bytes!(#migration_files);)*
            let _ = ::core::option_env!("SQLX_OFFLINE");
            let _ = ::core::option_env!("SQLX_OFFLINE_DIR");
        };
        #warning
    })
}

/// A provider this build cannot check.
fn check_provider(plan: &kubuno_data_model::TypedPlan, display: &str) -> Result<(), String> {
    use kubuno_data_model::ProviderName;
    let (available, feature) = match plan.provider {
        ProviderName::Sqlite => (cfg!(feature = "sqlite"), "sqlite"),
        ProviderName::Postgres => (cfg!(feature = "postgres"), "postgres"),
        ProviderName::Mysql => (cfg!(feature = "mysql"), "mysql"),
        ProviderName::Sqlserver => (cfg!(feature = "mssql"), "mssql"),
    };
    if available {
        Ok(())
    } else {
        Err(format!("`{display}`: provider `{}` needs the `{feature}` feature of kubuno-data (e.g. `kubuno-data = {{ features = [\"{feature}\"] }}`)", plan.provider))
    }
}

/// sqlx's expansion of one statement, its `sqlx` paths re-rooted at `krate`.
fn expand_sqlx(call: &SqlxCall<'_>, krate: &TokenStream2) -> Result<TokenStream2, String> {
    let sql = call.sql;
    let args = &call.args;
    let tokens = match &call.record {
        Some(record) => quote!(source = #sql, record = #record, args = [#(#args),*]),
        None => quote!(source = #sql, args = [#(#args),*]),
    };
    let input: sqlx_macros_core::query::QueryMacroInput = syn::parse2(tokens).map_err(|e| e.to_string())?;
    let expanded = sqlx_macros_core::query::expand_input(input, sqlx_macros_core::FOSS_DRIVERS).map_err(|e| e.to_string())?;
    Ok(rewrite::reroot_sqlx(expanded, krate))
}

/// Why the offline cache does not match the data source, when a build relies on it (never during a
/// cache regeneration, when `SQLX_OFFLINE_DIR` is set): a statement without cache file (the
/// `.kbdata` changed), or a migration newer than the statements' cache files (the schema changed).
fn stale_warning(plan: &kubuno_data_model::TypedPlan, manifest_dir: Option<&Path>, cache_dirs: &[PathBuf], display: &str) -> Option<String> {
    if std::env::var_os("SQLX_OFFLINE_DIR").is_some() || plan.provider == kubuno_data_model::ProviderName::Sqlserver {
        return None;
    }
    // Offline, sqlx itself refuses a missing statement (a compile error that says the same).
    let online = std::env::var_os("DATABASE_URL").is_some() && !std::env::var("SQLX_OFFLINE").is_ok_and(|v| v.eq_ignore_ascii_case("true") || v == "1");
    let reason = stale_reason(plan, manifest_dir?, cache_dirs, online)?;
    Some(format!("kubuno-data: `{display}`: {reason}"))
}

fn stale_reason(plan: &kubuno_data_model::TypedPlan, manifest_dir: &Path, cache_dirs: &[PathBuf], online: bool) -> Option<String> {
    let missing = kubuno_data_model::cache::missing_queries(plan, cache_dirs);
    if !missing.is_empty() && !online {
        return None;
    }
    let fix = "regenerate it: `cargo sqlx prepare`, or DATABASE_URL=<database> SQLX_OFFLINE=false SQLX_OFFLINE_DIR=<crate>/.sqlx cargo check (Visual Studio: Update the SQLx cache)";
    if !missing.is_empty() {
        let labels: Vec<&str> = missing.iter().map(|(l, _)| l.as_str()).take(4).collect();
        let more = if missing.len() > labels.len() { format!(" and {} more", missing.len() - labels.len()) } else { String::new() };
        return Some(format!("the offline query cache (.sqlx) has no entry for {}{more}: {fix}", labels.join(", ")));
    }
    // Every statement is cached: stale only if the schema changed after (a newer migration).
    let oldest_cache = plan
        .statements()
        .iter()
        .filter_map(|(_, st)| cache_dirs.iter().map(|d| d.join(st.cache_file_name())).find(|p| p.is_file()))
        .filter_map(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
        .min()?;
    let status = kubuno_data_model::cache::check(manifest_dir);
    match status.newest_input {
        Some((input, time)) if time > oldest_cache && input.extension().is_some_and(|e| e == "sql") => {
            let name = input.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Some(format!("the offline query cache (.sqlx) is older than the migration `{name}`: {fix}"))
        }
        _ => None,
    }
}

/// The workspace root above `manifest_dir` (the nearest `Cargo.toml` with a `[workspace]` table).
fn workspace_root(manifest_dir: &Path) -> Option<PathBuf> {
    manifest_dir.ancestors().find(|d| std::fs::read_to_string(d.join("Cargo.toml")).is_ok_and(|t| t.lines().any(|l| l.trim() == "[workspace]"))).map(Path::to_path_buf)
}

/// `file` relative to the package (`src/data/shop.kbdata`), for messages; `written` (the path as
/// written in the macro call) when the file is outside the package, so that messages never carry a
/// machine-specific absolute path.
fn display_path(file: &Path, manifest_dir: Option<&Path>, written: &str) -> String {
    match manifest_dir.and_then(|m| file.strip_prefix(m).ok()) {
        Some(rel) => rel.to_string_lossy().replace('\\', "/"),
        None => written.replace('\\', "/"),
    }
}

/// Resolves the `.kbdata` path like `include_str!`: relative to the file holding the macro call,
/// else (tools that do not know that file) to the package's `src` folder, then to its root.
fn resolve(path: &LitStr) -> Result<PathBuf, String> {
    let rel = PathBuf::from(path.value());
    if rel.is_absolute() {
        return if rel.is_file() { Ok(rel) } else { Err(format!("cannot find the data source `{}`", rel.display())) };
    }
    let mut tried = Vec::new();
    // The literal's span is the caller's (the `macro_rules!` forwarding keeps it), so its file is
    // the one that holds `data_source!`.
    if let Some(dir) = local_file(path.span()).and_then(|f| f.parent().map(Path::to_path_buf)) {
        if dir.is_relative() {
            if let Ok(cwd) = std::env::current_dir() {
                tried.push(cwd.join(&dir).join(&rel));
            }
            if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") {
                tried.push(PathBuf::from(manifest).join(&dir).join(&rel));
            }
        } else {
            tried.push(dir.join(&rel));
        }
    }
    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from) {
        tried.push(manifest.join("src").join(&rel));
        tried.push(manifest.join(&rel));
    }
    if let Some(found) = tried.iter().find(|p| p.is_file()) {
        return Ok(normalize(found));
    }
    // A tool that expands the macro without telling which file holds it (rust-analyzer's proc-macro
    // server has no `Span::local_file`): the one file under `src` whose path ends with the given one.
    if let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from) {
        if let Some(found) = unique_under(&manifest.join("src"), &rel)? {
            return Ok(normalize(&found));
        }
    }
    Err(format!("cannot find the data source `{}` (looked relative to the file holding data_source!, then in the package's `src` folder and root)", path.value()))
}

/// The one file under `root` (recursively) whose path ends with `rel` (component-wise); `None` when
/// there is none, an error naming them when several match.
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
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(_) => {
                    let components: Vec<_> = path.components().collect();
                    if components.len() >= wanted.len() && components[components.len() - wanted.len()..] == wanted[..] {
                        matches.push(path);
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
            Err(format!("`{}` matches several files under `src` ({}): give its path relative to the file holding data_source!", rel.display(), list.join(", ")))
        }
    }
}

fn local_file(span: Span) -> Option<PathBuf> {
    // `Span::unwrap` panics outside a procedural macro (unit tests): only called from the macro.
    span.unwrap().local_file()
}

/// `a/./b/../c` → `a/c`, lexically (no `canonicalize`: on a mapped network drive it would give a
/// `\\?\UNC\…` path that no longer starts with `CARGO_MANIFEST_DIR`).
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

#[cfg(test)]
mod tests {
    use super::*;

    /// rust-analyzer's proc-macro server gives no `Span::local_file`: `data_source!("shop.kbdata")` in
    /// `src/data/shop.rs` must still find `src/data/shop.kbdata`, by its unique match under `src`.
    #[test]
    fn a_source_is_found_under_src_without_the_calling_file() {
        let root = std::env::temp_dir().join(format!("kubuno-data-macros-unique-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("src/data")).expect("dirs");
        std::fs::create_dir_all(root.join("src/other")).expect("dirs");
        std::fs::write(root.join("src/data/shop.kbdata"), "x").expect("write");
        std::fs::write(root.join("src/data/sales.kbdata"), "x").expect("write");
        std::fs::write(root.join("src/other/sales.kbdata"), "x").expect("write");
        let src = root.join("src");
        assert_eq!(unique_under(&src, Path::new("shop.kbdata")), Ok(Some(src.join("data").join("shop.kbdata"))));
        assert_eq!(unique_under(&src, Path::new("./data/shop.kbdata")), Ok(Some(src.join("data").join("shop.kbdata"))));
        assert_eq!(unique_under(&src, Path::new("missing.kbdata")), Ok(None));
        let ambiguous = unique_under(&src, Path::new("sales.kbdata")).expect_err("two files");
        assert!(ambiguous.contains("several files"), "{ambiguous}");
        assert_eq!(unique_under(&src, Path::new("other/sales.kbdata")), Ok(Some(src.join("other").join("sales.kbdata"))));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn input_forms() {
        let direct: Input = syn::parse2(quote!("shop.kbdata")).expect("direct");
        assert_eq!(direct.path.value(), "shop.kbdata");
        assert_eq!(direct.krate.to_string(), ":: kubuno_data");
        let forwarded: Input = syn::parse2(quote!(krate = ::kubuno::data; "shop.kbdata",)).expect("forwarded");
        assert_eq!(forwarded.krate.to_string(), ":: kubuno :: data");
        assert!(syn::parse2::<Input>(quote!(shop)).is_err());
        assert!(syn::parse2::<Input>(quote!("a", "b")).is_err());
    }

    #[test]
    fn paths_for_messages() {
        let m = Path::new("C:/app");
        assert_eq!(display_path(Path::new("C:/app/src/data/shop.kbdata"), Some(m), "x"), "src/data/shop.kbdata");
        assert_eq!(display_path(Path::new("D:/elsewhere/shop.kbdata"), Some(m), r"..\elsewhere/shop.kbdata"), "../elsewhere/shop.kbdata");
        assert_eq!(normalize(Path::new("C:/app/tests/ui/../typed/./shop.kbdata")), PathBuf::from("C:/app/tests/typed/shop.kbdata"));
        assert_eq!(display_path(&normalize(Path::new("C:/app/tests/ui/../typed/shop.kbdata")), Some(m), "x"), "tests/typed/shop.kbdata");
    }

    #[test]
    fn stale_reasons() {
        let dir = std::env::temp_dir().join(format!("kubuno-data-macros-stale-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".sqlx")).expect("dir");
        let source = kubuno_data_model::DataSource::parse("name='s'\nconnection='s'\nprovider='sqlite'\n[[tables]]\nname='t'\ncolumns=[{name='a', db_type='TEXT', rust_type='String'}]").expect("parse");
        let plan = kubuno_data_model::plan(&source).expect("plan");
        let dirs = vec![dir.join(".sqlx")];
        assert_eq!(stale_reason(&plan, &dir, &dirs, false), None, "offline: sqlx reports it");
        let missing = stale_reason(&plan, &dir, &dirs, true).expect("missing");
        assert!(missing.contains("has no entry for T::fetch_all"), "{missing}");
        for (_, st) in plan.statements() {
            std::fs::write(dir.join(".sqlx").join(st.cache_file_name()), "{}").expect("write");
        }
        assert_eq!(stale_reason(&plan, &dir, &dirs, true), None);
        std::fs::create_dir_all(dir.join("migrations")).expect("migrations");
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        std::fs::write(dir.join("migrations/2_add.sql"), "x").expect("write");
        std::fs::File::options().write(true).open(dir.join("migrations/2_add.sql")).and_then(|f| f.set_modified(later)).expect("time");
        let migration = stale_reason(&plan, &dir, &dirs, true).expect("migration");
        assert!(migration.contains("older than the migration `2_add.sql`"), "{migration}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
