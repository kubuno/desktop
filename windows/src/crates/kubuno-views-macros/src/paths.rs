//! Where the generated code finds `kubuno_views`.
//!
//! The macros name `::kubuno_views::…`. An application that depends on the `kubuno` facade only
//! (the Kubuno templates: one Kubuno dependency) has no `kubuno_views` of its own — it reaches the
//! crate as `kubuno::views`. The expansion is then retargeted: every `::kubuno_views` path becomes
//! `::kubuno::views` (the approach of `proc-macro-crate`, reading the package's `Cargo.toml`).

use proc_macro2::{Group, Ident, Punct, Spacing, TokenStream as TokenStream2, TokenTree};

/// How the package being compiled reaches `kubuno_views`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ViewsPath {
    /// `::kubuno_views` (a direct dependency, or `kubuno_views` itself).
    Direct,
    /// `::kubuno::views` (through the facade).
    Facade,
}

/// Reads the package's manifest: `Facade` when it depends on `kubuno` and not on `kubuno-views`.
pub(crate) fn views_path() -> ViewsPath {
    let Some(dir) = std::env::var_os("CARGO_MANIFEST_DIR") else { return ViewsPath::Direct };
    let Ok(manifest) = std::fs::read_to_string(std::path::Path::new(&dir).join("Cargo.toml")) else { return ViewsPath::Direct };
    decide(&manifest)
}

/// The decision for a manifest's text (see [`views_path`]).
pub(crate) fn decide(manifest: &str) -> ViewsPath {
    let mut package = None;
    let mut direct = false;
    let mut facade = false;
    let mut section = String::new();
    for line in manifest.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
            section = header.trim().to_string();
            // `[dependencies.kubuno-views]` tables.
            let dep = section.rsplit_once('.').map(|(table, name)| (table, name.trim_matches('"')));
            if let Some((table, name)) = dep {
                if table.ends_with("dependencies") {
                    direct |= name == "kubuno-views" || name == "kubuno_views";
                    facade |= name == "kubuno";
                }
            }
            continue;
        }
        let Some((key, _)) = line.split_once('=') else { continue };
        let key = key.trim().trim_matches('"');
        if section == "package" && key == "name" {
            package = line.split_once('=').map(|(_, v)| v.trim().trim_matches('"').to_string());
        }
        if section.ends_with("dependencies") {
            direct |= key == "kubuno-views" || key == "kubuno_views";
            facade |= key == "kubuno";
        }
    }
    let own = package.as_deref() == Some("kubuno-views");
    if facade && !direct && !own {
        ViewsPath::Facade
    } else {
        ViewsPath::Direct
    }
}

/// The generated code with `::kubuno_views` paths rewritten for the package being compiled.
pub(crate) fn retarget(tokens: TokenStream2) -> TokenStream2 {
    match views_path() {
        ViewsPath::Direct => tokens,
        ViewsPath::Facade => rewrite(tokens),
    }
}

/// Rewrites every `:: kubuno_views` of `tokens` as `:: kubuno :: views`.
pub(crate) fn rewrite(tokens: TokenStream2) -> TokenStream2 {
    let mut out: Vec<TokenTree> = Vec::new();
    for tree in tokens {
        match tree {
            TokenTree::Group(g) => {
                let mut group = Group::new(g.delimiter(), rewrite(g.stream()));
                group.set_span(g.span());
                out.push(TokenTree::Group(group));
            }
            TokenTree::Ident(ident) if ident == "kubuno_views" && ends_with_path_separator(&out) => {
                let span = ident.span();
                out.push(TokenTree::Ident(Ident::new("kubuno", span)));
                let mut first = Punct::new(':', Spacing::Joint);
                first.set_span(span);
                let mut second = Punct::new(':', Spacing::Alone);
                second.set_span(span);
                out.push(TokenTree::Punct(first));
                out.push(TokenTree::Punct(second));
                out.push(TokenTree::Ident(Ident::new("views", span)));
            }
            other => out.push(other),
        }
    }
    out.into_iter().collect()
}

/// Whether the tokens so far end with `::` that starts a path to `kubuno_views` (not `crate::`,
/// `self::` or `super::`, a module of the package that would carry that name).
fn ends_with_path_separator(out: &[TokenTree]) -> bool {
    let n = out.len();
    let is_colon = |t: &TokenTree| matches!(t, TokenTree::Punct(p) if p.as_char() == ':');
    if n < 2 || !(is_colon(&out[n - 1]) && is_colon(&out[n - 2])) {
        return false;
    }
    !matches!(out.get(n.wrapping_sub(3)), Some(TokenTree::Ident(i)) if i == "crate" || i == "self" || i == "super")
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    #[test]
    fn the_facade_is_used_when_the_package_depends_on_kubuno_only() {
        assert_eq!(decide("[package]\nname = \"app\"\n\n[dependencies]\nkubuno = { path = \"x\" }\n"), ViewsPath::Facade);
        assert_eq!(decide("[package]\nname = \"app\"\n[dependencies]\nkubuno = { path = \"x\" }\nkubuno-views = { path = \"y\" }\n"), ViewsPath::Direct);
        assert_eq!(decide("[package]\nname = \"app\"\n[dependencies]\nkubuno-views = { path = \"y\" }\n"), ViewsPath::Direct);
        assert_eq!(decide("[package]\nname = \"app\"\n[dependencies.kubuno]\npath = \"x\"\n"), ViewsPath::Facade);
        assert_eq!(decide("[package]\nname = \"kubuno-views\"\n[dev-dependencies]\nkubuno = { path = \"x\" }\n"), ViewsPath::Direct);
        assert_eq!(decide("[package]\nname = \"app\"\n"), ViewsPath::Direct);
    }

    #[test]
    fn absolute_kubuno_views_paths_are_rewritten() {
        let out = rewrite(quote!(impl ::kubuno_views::events::EventArgs for X { fn f() -> ::kubuno_views::binding::Value { crate::kubuno_views::b() } }));
        let text = out.to_string();
        assert!(text.contains(":: kubuno :: views :: events :: EventArgs"), "{text}");
        assert!(text.contains(":: kubuno :: views :: binding :: Value"), "{text}");
        assert!(text.contains("crate :: kubuno_views :: b"), "a module of the package is kept: {text}");
    }
}
