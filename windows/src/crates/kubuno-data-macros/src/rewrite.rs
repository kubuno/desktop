//! Re-rooting of `sqlx` paths. sqlx's query expansion names its crate `::sqlx::…` (and the database
//! types `sqlx::sqlite::Sqlite`…), which would require a direct `sqlx` dependency in the application;
//! every such path is rewritten to `<krate>::sqlx::…` (`$crate::sqlx` through `kubuno_data::data_source!`).

use proc_macro2::{Group, Ident, Punct, Spacing, TokenStream, TokenTree};

fn is_colon(t: &TokenTree) -> bool {
    matches!(t, TokenTree::Punct(p) if p.as_char() == ':')
}

fn is_path_sep_at(tokens: &[TokenTree], i: usize) -> bool {
    matches!(tokens.get(i), Some(TokenTree::Punct(p)) if p.as_char() == ':' && p.spacing() == Spacing::Joint) && tokens.get(i + 1).is_some_and(is_colon)
}

/// Keywords that can precede a path (`as ::sqlx::X`, `impl sqlx::Y`…): the `::` after them starts it.
fn is_keyword(id: &Ident) -> bool {
    let s = id.to_string();
    matches!(
        s.as_str(),
        "as" | "for" | "impl" | "dyn" | "in" | "return" | "let" | "where" | "mut" | "ref" | "move" | "match" | "if" | "else" | "while" | "break" | "const" | "static" | "use" | "pub" | "type" | "fn" | "unsafe" | "async" | "await" | "yield"
    )
}

/// Rewrites every `sqlx::…` / `::sqlx::…` path of `tokens` to `<krate>::sqlx::…`.
pub fn reroot_sqlx(tokens: TokenStream, krate: &TokenStream) -> TokenStream {
    let input: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out: Vec<TokenTree> = Vec::with_capacity(input.len());
    for (i, t) in input.iter().enumerate() {
        match t {
            TokenTree::Group(g) => {
                let mut group = Group::new(g.delimiter(), reroot_sqlx(g.stream(), krate));
                group.set_span(g.span());
                out.push(TokenTree::Group(group));
            }
            TokenTree::Ident(id) if id == "sqlx" && is_path_sep_at(&input, i + 1) => {
                let n = out.len();
                let after_sep = n >= 2 && is_colon(&out[n - 2]) && is_colon(&out[n - 1]);
                if after_sep {
                    // `a::sqlx::…` is somebody else's module: leave it.
                    let nested = n >= 3 && matches!(&out[n - 3], TokenTree::Ident(b) if !is_keyword(b)) || n >= 3 && matches!(&out[n - 3], TokenTree::Punct(p) if p.as_char() == '>');
                    if nested {
                        out.push(t.clone());
                        continue;
                    }
                    out.truncate(n - 2);
                }
                out.extend(krate.clone());
                out.push(TokenTree::Punct(Punct::new(':', Spacing::Joint)));
                out.push(TokenTree::Punct(Punct::new(':', Spacing::Alone)));
                out.push(TokenTree::Ident(Ident::new("sqlx", id.span())));
            }
            other => out.push(other.clone()),
        }
    }
    out.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;

    fn show(ts: TokenStream) -> String {
        ts.to_string().replace(' ', "")
    }

    #[test]
    fn sqlx_paths_are_rerooted() {
        let krate = quote!(::kubuno_data);
        let ts = quote! {
            ::sqlx::__query_with_result::<sqlx::sqlite::Sqlite, _>("x", a).try_map(|row: sqlx::sqlite::SqliteRow| {
                use ::sqlx::Row as _;
                let q = <sqlx::sqlite::Sqlite as ::sqlx::database::Database>::Arguments::<'_>::default();
                let keep = other::sqlx::Thing;
                let v: Option<sqlx::types::chrono::NaiveDate> = None;
            })
        };
        let s = show(reroot_sqlx(ts, &krate));
        assert!(s.starts_with("::kubuno_data::sqlx::__query_with_result::<::kubuno_data::sqlx::sqlite::Sqlite,_>"), "{s}");
        assert!(s.contains("row:::kubuno_data::sqlx::sqlite::SqliteRow"), "{s}");
        assert!(s.contains("use::kubuno_data::sqlx::Rowas_;"), "{s}");
        assert!(s.contains("<::kubuno_data::sqlx::sqlite::Sqliteas::kubuno_data::sqlx::database::Database>"), "{s}");
        assert!(s.contains("other::sqlx::Thing"), "{s}");
        assert!(s.contains("Option<::kubuno_data::sqlx::types::chrono::NaiveDate>"), "{s}");
        assert!(!s.contains("kubuno_data::kubuno_data"), "{s}");
    }

    #[test]
    fn already_rooted_paths_and_other_idents_are_kept() {
        let krate = quote!(::kubuno_data);
        let ts = quote!(::kubuno_data::sqlx::Sqlite; let sqlx = 1; x.sqlx(););
        assert_eq!(show(reroot_sqlx(ts.clone(), &krate)), show(ts));
    }
}
