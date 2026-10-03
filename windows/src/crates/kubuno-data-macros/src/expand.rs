//! Code generation from a [`TypedPlan`]: row structs, their `TypedRow` impl, the typed data
//! functions and their `*_task` twins. The SQL statements are expanded by a caller-provided function
//! (sqlx's query expansion in the macro, a stub in the tests).

use kubuno_data_model::typed::{FieldPlan, QueryResult, RowPlan, Statement, TypedPlan};
use kubuno_data_model::{ObjectKind, ProviderName};
use proc_macro2::{Ident, Span, TokenStream};
use quote::{format_ident, quote, ToTokens};

use crate::rewrite::reroot_sqlx;

/// What the generated code needs to know about its invocation.
pub struct Ctx {
    /// The path of `kubuno_data` (`$crate` through `kubuno_data::data_source!`).
    pub krate: TokenStream,
    /// The `.kbdata` path shown in messages.
    pub display: String,
}

/// One statement to expand with sqlx (`query!` when `record` is `None`, else `query_as!`).
pub struct SqlxCall<'a> {
    /// `Customer::fetch_all`, `customers_by_city` (the macro prefixes errors with it itself).
    #[cfg_attr(not(test), allow(dead_code))]
    pub label: &'a str,
    pub sql: &'a str,
    pub record: Option<Ident>,
    pub args: Vec<TokenStream>,
}

/// Expands a statement into an expression (sqlx's `Map`/`Query`), or explains why it cannot.
pub type Expander<'a> = dyn FnMut(&SqlxCall<'_>) -> Result<TokenStream, String> + 'a;

/// An identifier (`r#type` → a raw identifier).
pub fn ident(s: &str) -> Ident {
    match s.strip_prefix("r#") {
        Some(raw) => Ident::new_raw(raw, Span::call_site()),
        None => Ident::new(s, Span::call_site()),
    }
}

/// Types every generated derive supports (`Default`, `PartialEq`); other types get `Debug`/`Clone` only.
fn is_known_type(ty: &str) -> bool {
    matches!(
        ty,
        "i8" | "i16"
            | "i32"
            | "i64"
            | "u8"
            | "u16"
            | "u32"
            | "u64"
            | "f32"
            | "f64"
            | "bool"
            | "String"
            | "Vec<u8>"
            | "sqlx::types::chrono::NaiveDate"
            | "sqlx::types::chrono::NaiveTime"
            | "sqlx::types::chrono::NaiveDateTime"
            | "sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc>"
            | "sqlx::types::Uuid"
            | "sqlx::types::JsonValue"
    )
}

struct Gen<'c, 'e, 'x> {
    ctx: &'c Ctx,
    plan: &'c TypedPlan,
    sqlx: &'e mut Expander<'x>,
    /// Statements sqlx refused: (label, error).
    failures: Vec<(String, String)>,
}

impl Gen<'_, '_, '_> {
    fn error(&self, message: &str) -> TokenStream {
        let text = format!("`{}`: {message}", self.ctx.display);
        quote!(::core::compile_error!(#text);)
    }

    fn rust_type(&self, ty: &str) -> Result<TokenStream, String> {
        let parsed: syn::Type = syn::parse_str(ty).map_err(|e| format!("`{ty}` is not a Rust type ({e})"))?;
        Ok(reroot_sqlx(parsed.to_token_stream(), &self.ctx.krate))
    }

    fn field_type(&self, f: &FieldPlan) -> Result<TokenStream, String> {
        let ty = self.rust_type(&f.rust_type).map_err(|e| format!("column `{}`: {e}", f.column))?;
        Ok(if f.nullable { quote!(::core::option::Option<#ty>) } else { ty })
    }

    /// The body of a data function: `let query = <sqlx expansion of st>; <rest>`. When sqlx refuses
    /// the statement, the failure is recorded (reported once per cause by [`generate`], naming
    /// every statement it hit) and the body is `unimplemented!()`, so nothing else errors.
    fn body(&mut self, label: &str, st: &Statement, record: Option<Ident>, arg: &dyn Fn(&str) -> TokenStream, rest: TokenStream) -> TokenStream {
        let call = SqlxCall { label, sql: &st.sql, record, args: st.args.iter().map(|a| arg(a)).collect() };
        match (self.sqlx)(&call) {
            Ok(q) => quote! {
                let query = #q;
                #rest
            },
            Err(e) => {
                self.failures.push((label.to_string(), e));
                quote!(::core::unimplemented!())
            }
        }
    }

    fn db(&self) -> TokenStream {
        let k = &self.ctx.krate;
        match self.plan.provider {
            ProviderName::Sqlite => quote!(#k::sqlx::Sqlite),
            ProviderName::Mysql => quote!(#k::sqlx::MySql),
            _ => quote!(#k::sqlx::Postgres),
        }
    }

    fn run(&self) -> Ident {
        match self.plan.provider {
            ProviderName::Sqlite => format_ident!("run_sqlite"),
            ProviderName::Mysql => format_ident!("run_mysql"),
            _ => format_ident!("run_pg"),
        }
    }

    /// The row struct, its consts and its `TypedRow` impl.
    fn row_struct(&self, row: &RowPlan, doc: &str) -> Result<TokenStream, String> {
        let k = &self.ctx.krate;
        let name = ident(&row.name);
        let mut fields = Vec::new();
        for f in &row.fields {
            let id = ident(&f.ident);
            let ty = self.field_type(f)?;
            let fdoc = format!("`{}` ({}{}{}).", f.column, f.db_type, if f.nullable { ", nullable" } else { "" }, if f.primary_key { ", key" } else { "" });
            fields.push(quote!(#[doc = #fdoc] pub #id: #ty));
        }
        let derives = if row.fields.iter().all(|f| is_known_type(&f.rust_type)) {
            quote!(#[derive(Debug, Clone, PartialEq, Default)])
        } else {
            quote!(#[derive(Debug, Clone)])
        };
        let source = &row.source;
        let columns: Vec<&str> = row.fields.iter().map(|f| f.column.as_str()).collect();
        let keys: Vec<&str> = row.key_fields().map(|f| f.column.as_str()).collect();
        let provider = self.plan.provider.as_str();
        let col_defs = row.fields.iter().map(|f| {
            let (c, t, n, pk, ai, ro, ml) = (&f.column, &f.db_type, f.nullable, f.primary_key, f.auto_increment, f.read_only, f.max_length);
            quote!(#k::typed::column(#provider, #c, #t, #n, #pk, #ai, #ro, #ml))
        });
        let ids: Vec<Ident> = row.fields.iter().map(|f| ident(&f.ident)).collect();
        let indices = 0..row.fields.len();
        let cols = columns.iter();
        Ok(quote! {
            #[doc = #doc]
            #derives
            pub struct #name {
                #(#fields,)*
            }

            impl #name {
                /// The table, view or query the rows come from.
                pub const TABLE: &str = #source;
                /// The column names, in field order.
                pub const COLUMNS: &[&str] = &[#(#columns),*];
                /// The key columns (empty: no key).
                pub const KEY: &[&str] = &[#(#keys),*];
            }

            impl #k::TypedRow for #name {
                fn table_name() -> &'static str {
                    #source
                }
                fn columns() -> ::std::vec::Vec<#k::DataColumn> {
                    ::std::vec![#(#col_defs),*]
                }
                fn to_values(&self) -> ::std::vec::Vec<#k::DbValue> {
                    ::std::vec![#(#k::typed::FieldValue::to_db(&self.#ids)),*]
                }
                fn from_values(values: &[#k::DbValue]) -> ::core::result::Result<Self, #k::DataError> {
                    ::core::result::Result::Ok(Self { #(#ids: #k::typed::field(values, #indices, #cols)?),* })
                }
            }
        })
    }

    /// The typed functions of a table or view (sqlx providers).
    fn row_functions(&mut self, row: &RowPlan) -> Result<TokenStream, String> {
        let k = self.ctx.krate.clone();
        let db = self.db();
        let run = self.run();
        let name = ident(&row.name);
        let err = quote!(#k::DataError);
        let mut out = TokenStream::new();
        let key_fields: Vec<&FieldPlan> = row.key_fields().collect();
        let key_ids: Vec<Ident> = key_fields.iter().map(|f| ident(&f.ident)).collect();
        let key_tys = key_fields.iter().map(|f| self.field_type(f)).collect::<Result<Vec<_>, _>>()?;
        let alias_to_ident = |alias: &str| row.field(alias).map(|f| ident(&f.ident)).unwrap_or_else(|| ident(alias));
        let self_arg = |alias: &str| {
            let id = alias_to_ident(alias);
            quote!(self.#id)
        };
        let key_arg = |alias: &str| {
            let id = alias_to_ident(alias);
            quote!(#id)
        };

        if let Some(st) = &row.select_all {
            let label = format!("{}::fetch_all", row.name);
            let body = self.body(&label, st, Some(name.clone()), &key_arg, quote!(query.fetch_all(executor).await.map_err(|e| #k::typed::db_error(#label, e))));
            let doc = format!("Every row of `{}`, checked at compile time against the offline cache.", row.source);
            out.extend(quote! {
                #[doc = #doc]
                #[allow(unused_variables)]
                pub async fn fetch_all<'e, E>(executor: E) -> ::core::result::Result<::std::vec::Vec<Self>, #err>
                where
                    E: #k::sqlx::Executor<'e, Database = #db>,
                {
                    #body
                }

                /// [`Self::fetch_all`] on the data runtime, from a connection component's handle: a task
                /// the UI awaits without blocking.
                pub fn fetch_all_task(conn: &#k::ConnectionHandle) -> #k::DataTask<::std::vec::Vec<Self>> {
                    conn.#run(move |pool| async move { Self::fetch_all(&pool).await })
                }
            });
        }
        if let Some(st) = &row.select_by_key {
            let label = format!("{}::fetch_by_key", row.name);
            let body = self.body(&label, st, Some(name.clone()), &key_arg, quote!(query.fetch_optional(executor).await.map_err(|e| #k::typed::db_error(#label, e))));
            out.extend(quote! {
                /// The row with this key, if any.
                #[allow(unused_variables)]
                pub async fn fetch_by_key<'e, E>(executor: E, #(#key_ids: #key_tys),*) -> ::core::result::Result<::core::option::Option<Self>, #err>
                where
                    E: #k::sqlx::Executor<'e, Database = #db>,
                {
                    #body
                }

                /// [`Self::fetch_by_key`] on the data runtime.
                pub fn fetch_by_key_task(conn: &#k::ConnectionHandle, #(#key_ids: #key_tys),*) -> #k::DataTask<::core::option::Option<Self>> {
                    conn.#run(move |pool| async move { Self::fetch_by_key(&pool, #(#key_ids),*).await })
                }
            });
        }
        if let Some(st) = &row.insert {
            let label = format!("{}::insert", row.name);
            if self.plan.provider == ProviderName::Mysql {
                let body = self.body(&label, st, None, &self_arg, quote! {
                    let done = query.execute(executor).await.map_err(|e| #k::typed::db_error(#label, e))?;
                    ::core::result::Result::Ok(done.last_insert_id())
                });
                out.extend(quote! {
                    /// Inserts the row (generated columns are left to the database); the generated id
                    /// (`LAST_INSERT_ID()`, 0 when the table has none).
                    #[allow(unused_variables)]
                    pub async fn insert<'e, E>(&self, executor: E) -> ::core::result::Result<u64, #err>
                    where
                        E: #k::sqlx::Executor<'e, Database = #db>,
                    {
                        #body
                    }

                    /// [`Self::insert`] on the data runtime (the row is cloned).
                    pub fn insert_task(&self, conn: &#k::ConnectionHandle) -> #k::DataTask<u64> {
                        let row = ::core::clone::Clone::clone(self);
                        conn.#run(move |pool| async move { row.insert(&pool).await })
                    }
                });
            } else {
                let body = self.body(&label, st, Some(name.clone()), &self_arg, quote!(query.fetch_one(executor).await.map_err(|e| #k::typed::db_error(#label, e))));
                out.extend(quote! {
                    /// Inserts the row (generated columns are left to the database) and returns it as
                    /// stored, generated key included (`RETURNING`).
                    #[allow(unused_variables)]
                    pub async fn insert<'e, E>(&self, executor: E) -> ::core::result::Result<Self, #err>
                    where
                        E: #k::sqlx::Executor<'e, Database = #db>,
                    {
                        #body
                    }

                    /// [`Self::insert`] on the data runtime (the row is cloned).
                    pub fn insert_task(&self, conn: &#k::ConnectionHandle) -> #k::DataTask<Self> {
                        let row = ::core::clone::Clone::clone(self);
                        conn.#run(move |pool| async move { row.insert(&pool).await })
                    }
                });
            }
        }
        if let Some(st) = &row.update {
            let label = format!("{}::update", row.name);
            let body = self.body(&label, st, None, &self_arg, quote! {
                let done = query.execute(executor).await.map_err(|e| #k::typed::db_error(#label, e))?;
                ::core::result::Result::Ok(done.rows_affected())
            });
            out.extend(quote! {
                /// Writes the row's non-key columns to the row with its key; the number of rows
                /// affected (0: nobody has this key any more).
                #[allow(unused_variables)]
                pub async fn update<'e, E>(&self, executor: E) -> ::core::result::Result<u64, #err>
                where
                    E: #k::sqlx::Executor<'e, Database = #db>,
                {
                    #body
                }

                /// [`Self::update`] on the data runtime (the row is cloned).
                pub fn update_task(&self, conn: &#k::ConnectionHandle) -> #k::DataTask<u64> {
                    let row = ::core::clone::Clone::clone(self);
                    conn.#run(move |pool| async move { row.update(&pool).await })
                }
            });
        }
        if let Some(st) = &row.delete {
            let label = format!("{}::delete", row.name);
            let body = self.body(&label, st, None, &key_arg, quote! {
                let done = query.execute(executor).await.map_err(|e| #k::typed::db_error(#label, e))?;
                ::core::result::Result::Ok(done.rows_affected())
            });
            out.extend(quote! {
                /// Deletes the row with this key; the number of rows affected.
                #[allow(unused_variables)]
                pub async fn delete_by_key<'e, E>(executor: E, #(#key_ids: #key_tys),*) -> ::core::result::Result<u64, #err>
                where
                    E: #k::sqlx::Executor<'e, Database = #db>,
                {
                    #body
                }

                /// Deletes this row (by its key); the number of rows affected.
                pub async fn delete<'e, E>(&self, executor: E) -> ::core::result::Result<u64, #err>
                where
                    E: #k::sqlx::Executor<'e, Database = #db>,
                {
                    Self::delete_by_key(executor, #(::core::clone::Clone::clone(&self.#key_ids)),*).await
                }

                /// [`Self::delete`] on the data runtime.
                pub fn delete_task(&self, conn: &#k::ConnectionHandle) -> #k::DataTask<u64> {
                    #(let #key_ids = ::core::clone::Clone::clone(&self.#key_ids);)*
                    conn.#run(move |pool| async move { Self::delete_by_key(&pool, #(#key_ids),*).await })
                }
            });
        }
        Ok(quote! {
            impl #name {
                #out
            }
        })
    }

    /// The run-time checked functions of a table or view (SQL Server, through `DbCommand`).
    fn row_functions_runtime(&self, row: &RowPlan) -> Result<TokenStream, String> {
        let k = &self.ctx.krate;
        let name = ident(&row.name);
        let mut out = TokenStream::new();
        if let Some(st) = &row.select_all {
            let sql = &st.sql;
            out.extend(quote! {
                /// Every row (SQL Server: run-time checked, through `DbCommand`).
                pub fn fetch_all_task(conn: &#k::ConnectionHandle) -> #k::DataTask<::std::vec::Vec<Self>> {
                    #k::typed::query_rows::<Self>(conn, #sql, ::std::vec::Vec::new())
                }
            });
        }
        if let Some(st) = &row.select_by_key {
            let sql = &st.sql;
            let key_fields: Vec<&FieldPlan> = row.key_fields().collect();
            let key_ids: Vec<Ident> = key_fields.iter().map(|f| ident(&f.ident)).collect();
            let key_tys = key_fields.iter().map(|f| self.field_type(f)).collect::<Result<Vec<_>, _>>()?;
            let aliases: Vec<&str> = key_fields.iter().map(|f| f.alias.as_str()).collect();
            out.extend(quote! {
                /// The row with this key, if any (SQL Server: run-time checked).
                pub fn fetch_by_key_task(conn: &#k::ConnectionHandle, #(#key_ids: #key_tys),*) -> #k::DataTask<::core::option::Option<Self>> {
                    #k::typed::first(#k::typed::query_rows::<Self>(conn, #sql, ::std::vec![#((#aliases, #k::typed::FieldValue::to_db(&#key_ids))),*]))
                }
            });
        }
        Ok(quote!(impl #name { #out }))
    }

    fn query(&mut self, q: &kubuno_data_model::QueryPlan) -> Result<TokenStream, String> {
        let k = self.ctx.krate.clone();
        let f = ident(&q.function);
        let f_task = format_ident!("{}_task", q.function.trim_start_matches("r#"));
        let p_ids: Vec<Ident> = q.params.iter().map(|p| ident(&p.ident)).collect();
        let p_tys = q.params.iter().map(|p| self.rust_type(&p.rust_type).map_err(|e| format!("query `{}`, parameter `{}`: {e}", q.name, p.name))).collect::<Result<Vec<_>, _>>()?;
        for p in &q.params {
            if matches!(p.ident.as_str(), "executor" | "conn" | "query" | "pool") {
                return Err(format!("query `{}`: `{}` is reserved, rename the parameter", q.name, p.name));
            }
        }
        let doc = format!("The query `{}` of the data source: `{}`", q.name, q.statement.sql);
        let mut out = TokenStream::new();
        if let QueryResult::OwnRow(row) = &q.result {
            let rdoc = format!("A row of the query `{}`.", q.name);
            out.extend(self.row_struct(row, &rdoc)?);
        }
        let row_ty = q.row_name().map(ident);
        let arg = |name: &str| {
            let id = q.param(name).map(|p| ident(&p.ident)).unwrap_or_else(|| ident(name));
            quote!(#id)
        };
        if self.plan.provider == ProviderName::Sqlserver {
            let sql = &q.statement.sql;
            let names: Vec<&str> = q.params.iter().map(|p| p.name.as_str()).collect();
            let params = quote!(::std::vec![#((#names, #k::typed::FieldValue::to_db(&#p_ids))),*]);
            out.extend(match &row_ty {
                Some(row) => quote! {
                    #[doc = #doc]
                #[allow(clippy::too_many_arguments, unused_variables)]
                    pub fn #f_task(conn: &#k::ConnectionHandle, #(#p_ids: #p_tys),*) -> #k::DataTask<::std::vec::Vec<#row>> {
                        #k::typed::query_rows::<#row>(conn, #sql, #params)
                    }
                },
                None => quote! {
                    #[doc = #doc]
                #[allow(clippy::too_many_arguments, unused_variables)]
                    pub fn #f_task(conn: &#k::ConnectionHandle, #(#p_ids: #p_tys),*) -> #k::DataTask<u64> {
                        #k::typed::execute(conn, #sql, #params)
                    }
                },
            });
            return Ok(out);
        }
        let db = self.db();
        let run = self.run();
        let label = q.function.trim_start_matches("r#").to_string();
        let (rows_rest, exec_rest) = (
            quote!(query.fetch_all(executor).await.map_err(|e| #k::typed::db_error(#label, e))),
            quote! {
                let done = query.execute(executor).await.map_err(|e| #k::typed::db_error(#label, e))?;
                ::core::result::Result::Ok(done.rows_affected())
            },
        );
        let body = self.body(&label, &q.statement, row_ty.clone(), &arg, if row_ty.is_some() { rows_rest } else { exec_rest });
        out.extend(match &row_ty {
            Some(row) => quote! {
                #[doc = #doc]
                #[allow(clippy::too_many_arguments, unused_variables)]
                pub async fn #f<'e, E>(executor: E, #(#p_ids: #p_tys),*) -> ::core::result::Result<::std::vec::Vec<#row>, #k::DataError>
                where
                    E: #k::sqlx::Executor<'e, Database = #db>,
                {
                    #body
                }

                #[doc = concat!("[`", stringify!(#f), "`] on the data runtime, from a connection component's handle.")]
                #[allow(clippy::too_many_arguments)]
                pub fn #f_task(conn: &#k::ConnectionHandle, #(#p_ids: #p_tys),*) -> #k::DataTask<::std::vec::Vec<#row>> {
                    conn.#run(move |pool| async move { #f(&pool, #(#p_ids),*).await })
                }
            },
            None => quote! {
                #[doc = #doc]
                #[allow(clippy::too_many_arguments, unused_variables)]
                pub async fn #f<'e, E>(executor: E, #(#p_ids: #p_tys),*) -> ::core::result::Result<u64, #k::DataError>
                where
                    E: #k::sqlx::Executor<'e, Database = #db>,
                {
                    #body
                }

                #[doc = concat!("[`", stringify!(#f), "`] on the data runtime, from a connection component's handle.")]
                #[allow(clippy::too_many_arguments)]
                pub fn #f_task(conn: &#k::ConnectionHandle, #(#p_ids: #p_tys),*) -> #k::DataTask<u64> {
                    conn.#run(move |pool| async move { #f(&pool, #(#p_ids),*).await })
                }
            },
        });
        Ok(out)
    }
}

/// What to do about an error sqlx reported for a statement.
fn hint(e: &str) -> &'static str {
    if e.contains("no cached data") || e.contains("set `DATABASE_URL`") {
        " — not in the offline query cache (.sqlx): the .kbdata (or the schema) changed since the cache was prepared, or it never was. Regenerate the cache against a database that has the schema: set DATABASE_URL, SQLX_OFFLINE=false and SQLX_OFFLINE_DIR=<the crate's absolute path>/.sqlx (an existing folder), then `cargo check` (Visual Studio: Update the SQLx cache)"
    } else if e.contains("no such column") || e.contains("no such table") || e.contains("does not exist") || e.contains("Unknown column") {
        " — the .kbdata does not match the database schema"
    } else {
        ""
    }
}

/// The items of a data source.
pub fn generate(plan: &TypedPlan, ctx: &Ctx, sqlx: &mut Expander<'_>) -> Result<TokenStream, String> {
    let mut g = Gen { ctx, plan, sqlx, failures: Vec::new() };
    let mut out = TokenStream::new();
    for row in &plan.rows {
        let what = if row.kind == ObjectKind::View { "view" } else { "table" };
        let doc = format!("A row of the {what} `{}` (data source `{}`).", row.source, ctx.display);
        out.extend(g.row_struct(row, &doc).map_err(|e| format!("{what} `{}`: {e}", row.source))?);
        let functions = if plan.provider == ProviderName::Sqlserver { g.row_functions_runtime(row) } else { g.row_functions(row) };
        out.extend(functions.map_err(|e| format!("{what} `{}`: {e}", row.source))?);
    }
    for q in &plan.queries {
        out.extend(g.query(q)?);
    }
    // One error per cause, naming every statement it hit (a missing cache would otherwise repeat
    // the same message for each statement).
    let mut causes: Vec<(String, Vec<String>)> = Vec::new();
    for (label, e) in std::mem::take(&mut g.failures) {
        match causes.iter_mut().find(|(c, _)| *c == e) {
            Some((_, labels)) => labels.push(label),
            None => causes.push((e, vec![label])),
        }
    }
    for (e, labels) in causes {
        out.extend(g.error(&format!("{}: {e}{}", labels.join(", "), hint(&e))));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kubuno_data_model::DataSource;

    const SHOP: &str = r#"
name = "Shop"
connection = "Shop"
provider = "sqlite"

[[tables]]
name = "customers"
key = ["id"]
columns = [
  { name = "id", db_type = "INTEGER", rust_type = "i64", auto_increment = true },
  { name = "name", db_type = "TEXT", rust_type = "String" },
  { name = "type", db_type = "TEXT", rust_type = "String", nullable = true },
  { name = "born", db_type = "DATE", rust_type = "chrono::NaiveDate", nullable = true },
]

[[tables]]
name = "top_customers"
kind = "view"
columns = [{ name = "name", db_type = "TEXT", rust_type = "String" }]

[[queries]]
name = "by_name"
sql = "SELECT * FROM customers WHERE name = @name"
row = "Customer"
params = [{ name = "name", rust_type = "String" }]

[[queries]]
name = "forget"
sql = "DELETE FROM customers WHERE id = @id"
params = [{ name = "id", rust_type = "i64" }]
"#;

    /// A recorded stub call: label, SQL, record type, arguments.
    type Call = (String, String, Option<String>, Vec<String>);

    fn show(ts: &TokenStream) -> String {
        ts.to_string().replace(' ', "")
    }

    /// Generates with a stub expander that records the calls.
    fn gen(text: &str) -> (Result<TokenStream, String>, Vec<Call>) {
        let plan = kubuno_data_model::plan(&DataSource::parse(text).expect("parses")).expect("plan");
        let ctx = Ctx { krate: quote!(::kubuno_data), display: "src/shop.kbdata".into() };
        let mut calls = Vec::new();
        let mut stub = |c: &SqlxCall<'_>| {
            calls.push((c.label.to_string(), c.sql.to_string(), c.record.as_ref().map(|r| r.to_string()), c.args.iter().map(|a| a.to_string().replace(' ', "")).collect()));
            if c.sql.contains("DELETE FROM customers") {
                Err("error returned from database: no such table: customers".to_string())
            } else {
                Ok(quote!(sqlx_stub()))
            }
        };
        let out = generate(&plan, &ctx, &mut stub);
        (out, calls)
    }

    #[test]
    fn a_table_becomes_a_struct_with_typed_functions() {
        let (out, calls) = gen(SHOP);
        let s = show(&out.expect("generates"));
        assert!(s.contains("#[derive(Debug,Clone,PartialEq,Default)]pubstructCustomer{"), "{s}");
        assert!(s.contains("pubid:i64"), "{s}");
        assert!(s.contains("pubr#type:::core::option::Option<String>"), "{s}");
        assert!(s.contains("pubborn:::core::option::Option<::kubuno_data::sqlx::types::chrono::NaiveDate>"), "{s}");
        assert!(s.contains("pubconstTABLE:&str=\"customers\";"), "{s}");
        assert!(s.contains("pubconstCOLUMNS:&[&str]=&[\"id\",\"name\",\"type\",\"born\"];"), "{s}");
        assert!(s.contains("pubconstKEY:&[&str]=&[\"id\"];"), "{s}");
        assert!(s.contains("impl::kubuno_data::TypedRowforCustomer"), "{s}");
        assert!(s.contains("::kubuno_data::typed::column(\"sqlite\",\"id\",\"INTEGER\",false,true,true,false,0u32)"), "{s}");
        assert!(s.contains("r#type:::kubuno_data::typed::field(values,2usize,\"type\")?"), "{s}");
        for f in ["fnfetch_all<'e,E>(executor:E)", "fnfetch_all_task(conn:&::kubuno_data::ConnectionHandle)", "fnfetch_by_key<'e,E>(executor:E,id:i64)", "fninsert<'e,E>(&self,executor:E)->::core::result::Result<Self,", "fnupdate<'e,E>(&self,executor:E)->::core::result::Result<u64,", "fndelete_by_key<'e,E>(executor:E,id:i64)", "fndelete<'e,E>(&self,executor:E)", "fninsert_task(&self,conn:&::kubuno_data::ConnectionHandle)->::kubuno_data::DataTask<Self>"] {
            assert!(s.contains(f), "{f} in {s}");
        }
        assert!(s.contains("E:::kubuno_data::sqlx::Executor<'e,Database=::kubuno_data::sqlx::Sqlite>"), "{s}");
        assert!(s.contains("conn.run_sqlite(move|pool|asyncmove{Self::fetch_all(&pool).await})"), "{s}");
        // The statements handed to sqlx: typed columns, `?n` placeholders, the args in order.
        let fetch = calls.iter().find(|c| c.0 == "Customer::fetch_all").expect("fetch_all");
        assert_eq!(fetch.1, "SELECT \"id\" AS \"id!: i64\", \"name\" AS \"name!: String\", \"type\" AS \"type?: String\", \"born\" AS \"born?: sqlx::types::chrono::NaiveDate\" FROM \"customers\" ORDER BY \"id\"");
        assert_eq!(fetch.2.as_deref(), Some("Customer"));
        let insert = calls.iter().find(|c| c.0 == "Customer::insert").expect("insert");
        assert_eq!(insert.3, ["self.name", "self.r#type", "self.born"]);
        let update = calls.iter().find(|c| c.0 == "Customer::update").expect("update");
        assert_eq!(update.2, None);
        assert_eq!(update.3, ["self.name", "self.r#type", "self.born", "self.id"]);
        let by_key = calls.iter().find(|c| c.0 == "Customer::fetch_by_key").expect("by key");
        assert_eq!(by_key.3, ["id"]);
    }

    #[test]
    fn a_view_has_reads_only() {
        let (out, calls) = gen(SHOP);
        let s = show(&out.expect("generates"));
        assert!(s.contains("pubstructTopCustomer{"), "{s}");
        assert!(calls.iter().any(|c| c.0 == "TopCustomer::fetch_all"));
        assert!(!calls.iter().any(|c| c.0.starts_with("TopCustomer::") && c.0 != "TopCustomer::fetch_all"));
        assert!(s.contains("pubconstKEY:&[&str]=&[];"), "{s}");
    }

    #[test]
    fn queries_become_functions_and_errors_name_the_kbdata() {
        let (out, calls) = gen(SHOP);
        let s = show(&out.expect("generates"));
        assert!(s.contains("pubasyncfnby_name<'e,E>(executor:E,name:String)->::core::result::Result<::std::vec::Vec<Customer>,::kubuno_data::DataError>"), "{s}");
        assert!(s.contains("pubfnby_name_task(conn:&::kubuno_data::ConnectionHandle,name:String)"), "{s}");
        let q = calls.iter().find(|c| c.0 == "by_name").expect("query");
        assert!(q.1.ends_with("FROM (SELECT * FROM customers WHERE name = ?1) AS \"kubuno_q\""), "{}", q.1);
        assert_eq!(q.3, ["name"]);
        // The stub failed `forget`: a compile error naming the file, the query and the cause.
        assert!(s.contains("::core::compile_error!(\"`src/shop.kbdata`:forget:errorreturnedfromdatabase:nosuchtable:customers—the.kbdatadoesnotmatchthedatabaseschema\")"), "{s}");
        assert!(s.contains("pubasyncfnforget<'e,E>(executor:E,id:i64)->::core::result::Result<u64,"), "{s}");
    }

    #[test]
    fn postgres_and_sqlserver() {
        let pg = SHOP.replace("provider = \"sqlite\"", "provider = \"postgres\"\nschema = \"shop\"");
        let (out, calls) = gen(&pg);
        let s = show(&out.expect("generates"));
        assert!(s.contains("Database=::kubuno_data::sqlx::Postgres"), "{s}");
        assert!(s.contains("conn.run_pg("), "{s}");
        assert!(calls.iter().any(|c| c.1.contains("FROM \"shop\".\"customers\" WHERE \"id\" = $1")));

        let ms = SHOP.replace("provider = \"sqlite\"", "provider = \"sqlserver\"");
        let (out, calls) = gen(&ms);
        let s = show(&out.expect("generates"));
        assert!(calls.is_empty(), "no sqlx expansion for SQL Server");
        assert!(s.contains("::kubuno_data::typed::query_rows::<Self>(conn,\"SELECT[id],[name],[type],[born]FROM[customers]ORDERBY[id]\""), "{s}");
        assert!(s.contains("pubfnby_name_task(conn:&::kubuno_data::ConnectionHandle,name:String)->::kubuno_data::DataTask<::std::vec::Vec<Customer>>"), "{s}");
        assert!(s.contains("::kubuno_data::typed::execute(conn,\"DELETEFROMcustomersWHEREid=@id\""), "{s}");
        assert!(!s.contains("fninsert"), "{s}");
    }

    #[test]
    fn unknown_types_drop_the_derives_that_need_them() {
        let text = SHOP.replace("rust_type = \"chrono::NaiveDate\"", "rust_type = \"my::Date\"");
        let (out, _) = gen(&text);
        let s = show(&out.expect("generates"));
        assert!(s.contains("#[derive(Debug,Clone)]pubstructCustomer"), "{s}");
        let bad = SHOP.replace("rust_type = \"chrono::NaiveDate\"", "rust_type = \"Vec<\"");
        let (out, _) = gen(&bad);
        let e = out.expect_err("bad type");
        assert!(e.starts_with("table `customers`: column `born`: `Vec<` is not a Rust type"), "{e}");
    }
}
