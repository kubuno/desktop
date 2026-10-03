//! The **binding source schema** types of a view (`vskubuno/docs/DESIGNER.md`, "Data bindings"): what a
//! `{Binding …}` of an element can name — the data context, the item of a template, the named data
//! components, the resources and the converters — and how a path resolves against them.
//!
//! Moved here from `kubuno-views-ls` (`binding_sources`) by WV-1 so a web profile can answer from the
//! same types. The scanning that fills them (Rust code-behind, sample files) stays in the language
//! server. Locations are generic: `L` is where a member is declared and `U` the file of a context
//! (the language server uses `lsp_types::Location` and `lsp_types::Uri`); both only need
//! `Serialize` to be sent.

use serde::Serialize;


/// The shape of a value, as a property sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Shape {
    Bool,
    Number,
    Text,
    List,
    Object,
    Any,
}

impl Shape {
    /// The shape of a Rust type, as written.
    pub fn of_rust(ty: &str) -> Shape {
        let ty = ty.trim();
        if let Some(inner) = ty.strip_prefix("Option<").and_then(|t| t.strip_suffix('>')) {
            return Shape::of_rust(inner);
        }
        let last = ty.rsplit("::").next().unwrap_or(ty).trim();
        let head = last.split('<').next().unwrap_or(last).trim();
        match head {
            "bool" => Shape::Bool,
            "f32" | "f64" | "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize" => Shape::Number,
            "String" | "str" | "&str" | "Cow" | "Color" | "ColorValue" | "char" => Shape::Text,
            "Rows" | "Vec" | "VecDeque" => Shape::List,
            "Shared" | "Arc" | "Rc" | "ObjectValue" => Shape::Object,
            _ if ty.starts_with('&') => Shape::of_rust(ty.trim_start_matches('&').trim_start_matches("'static").trim()),
            _ => Shape::Any,
        }
    }

    /// The shape a `Value::Variant` constructor gives.
    pub fn of_value_variant(variant: &str) -> Shape {
        match variant {
            "Str" => Shape::Text,
            "F32" => Shape::Number,
            "Bool" => Shape::Bool,
            "List" => Shape::List,
            "Object" => Shape::Object,
            _ => Shape::Any,
        }
    }

    /// The shape of a JSON sample value.
    pub fn of_json(v: &serde_json::Value) -> Shape {
        match v {
            serde_json::Value::Bool(_) => Shape::Bool,
            serde_json::Value::Number(_) => Shape::Number,
            serde_json::Value::String(_) => Shape::Text,
            serde_json::Value::Array(_) => Shape::List,
            serde_json::Value::Object(_) => Shape::Object,
            serde_json::Value::Null => Shape::Any,
        }
    }

    /// The shape a converter's `output` names (`"Bool"`…).
    pub fn of_name(name: &str) -> Shape {
        match name {
            "Bool" => Shape::Bool,
            "Number" => Shape::Number,
            "Text" => Shape::Text,
            "List" => Shape::List,
            "Object" => Shape::Object,
            _ => Shape::Any,
        }
    }
}

/// What a member is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum MemberKind {
    /// A `#[bind]` field of a form class.
    Field,
    /// A `#[property]` of a user control.
    Property,
    /// An arm of an `impl ViewModel`'s `fn get`.
    Path,
    /// A field of an item's row.
    RowField,
    /// A named data component (`Source=`).
    Component,
    /// A column of a binding source.
    Column,
    /// A navigation/state member of a data component (`Position`, `Count`…).
    State,
    /// A `.kbres` resource.
    Resource,
}

/// One thing a binding can name.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Member<L> {
    /// The name shown (`Title`).
    pub name: String,
    /// The binding path (`Title`, `customers.Name`); for a resource, its key.
    pub path: String,
    /// What to write in the attribute to bind it (`{Binding Title}`, `{Binding Source=customers,
    /// Path=Name}`, `{Res title}`).
    pub expression: String,
    pub kind: MemberKind,
    /// The Rust type as written (`String`, `Rows`), when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rust_type: Option<String>,
    pub shape: Shape,
    /// Whether a two-way binding can write it (`false` for a read-only state, a resource).
    pub writable: bool,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub doc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<L>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Member<L>>,
}

impl<L> Member<L> {
    pub fn new(name: &str, path: &str, kind: MemberKind, shape: Shape) -> Self {
        Self {
            name: name.to_string(),
            path: path.to_string(),
            expression: format!("{{Binding {path}}}"),
            kind,
            rust_type: None,
            shape,
            writable: true,
            doc: String::new(),
            location: None,
            children: Vec::new(),
        }
    }
}

/// The members one level of resolution answers.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Context<L, U> {
    /// What answers (`StorageSection`, `Row of Blocks`).
    pub label: String,
    pub members: Vec<Member<L>>,
    /// Whether it may answer paths not listed (see the module doc).
    pub open: bool,
    /// The file that declares it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<U>,
}

impl<L, U> Context<L, U> {
    pub fn find(&self, path: &str) -> Option<&Member<L>> {
        self.members.iter().find(|m| m.path == path)
    }
}

/// A converter a binding can name.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConverterInfo<L> {
    pub name: String,
    pub output: Shape,
    pub two_way: bool,
    pub doc: String,
    /// Declared by the project (not built in).
    pub project: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location: Option<L>,
}

/// Everything a binding of one element can name.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Schema<L, U> {
    /// The view's data context.
    pub context: Context<L, U>,
    /// The row of the template the element is in, if it is in one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub item: Option<Context<L, U>>,
    /// The named data components (`Source=`), with their members as children.
    pub components: Vec<Member<L>>,
    pub resources: Vec<Member<L>>,
    pub converters: Vec<ConverterInfo<L>>,
}

/// How a path resolves.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolution<'a, L> {
    /// A member answers it.
    Found(&'a Member<L>),
    /// The scan cannot tell (an open context, a nested path of a list or object, a component
    /// whose columns are unknown).
    Unknown,
    /// Nothing answers it, and every level that could is closed.
    Missing,
}

impl<L, U> Schema<L, U> {
    /// Resolves `path` the way the runtime does: the item's row first, then the components
    /// (`customers.Name`), then the data context.
    pub fn resolve(&self, path: &str) -> Resolution<'_, L> {
        if let Some(m) = self.item.as_ref().and_then(|i| i.find(path)) {
            return Resolution::Found(m);
        }
        let (head, rest) = match path.split_once('.') {
            Some((h, r)) => (h, Some(r)),
            None => (path, None),
        };
        if let Some(c) = self.components.iter().find(|c| c.path == head) {
            return match rest {
                None => Resolution::Found(c),
                Some(rest) => {
                    // `customers.Current.Name` is `customers.Name`.
                    let rest = rest.strip_prefix("Current.").unwrap_or(rest);
                    let full = format!("{head}.{rest}");
                    match c.children.iter().find(|m| m.path == full || m.name == rest) {
                        Some(m) => Resolution::Found(m),
                        // `errors.Email.HasError`, or a binding source whose columns are unknown.
                        None if rest.contains('.') || c.children.iter().all(|m| m.kind != MemberKind::Column) => Resolution::Unknown,
                        None => Resolution::Missing,
                    }
                }
            };
        }
        if let Some(m) = self.context.find(path) {
            return Resolution::Found(m);
        }
        // `prefs.font`: a member holding a list or an object (or of an unknown type) answers deeper paths.
        if let Some(rest_head) = rest.and(self.context.find(head).or_else(|| self.item.as_ref().and_then(|i| i.find(head)))) {
            if matches!(rest_head.shape, Shape::List | Shape::Object | Shape::Any) {
                return Resolution::Unknown;
            }
        }
        let item_open = self.item.as_ref().is_some_and(|i| i.open);
        if self.context.open || item_open {
            Resolution::Unknown
        } else {
            Resolution::Missing
        }
    }

    /// Every member a path can name in this element (item first, then the context), flattened
    /// with the components' children.
    pub fn all_members(&self) -> Vec<&Member<L>> {
        let mut out: Vec<&Member<L>> = Vec::new();
        if let Some(i) = &self.item {
            out.extend(i.members.iter());
        }
        out.extend(self.context.members.iter());
        for c in &self.components {
            out.push(c);
            out.extend(c.children.iter());
        }
        out
    }

    /// The converter named `name`.
    pub fn converter(&self, name: &str) -> Option<&ConverterInfo<L>> {
        self.converters.iter().find(|c| c.name == name)
    }
}

// `Default` by hand: a derive would require `L: Default` and `U: Default`, which locations need not be.
impl<L, U> Default for Context<L, U> {
    fn default() -> Self {
        Self { label: String::new(), members: Vec::new(), open: false, file: None }
    }
}

impl<L, U> Default for Schema<L, U> {
    fn default() -> Self {
        Self { context: Context::default(), item: None, components: Vec::new(), resources: Vec::new(), converters: Vec::new() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type TestSchema = Schema<String, String>;

    fn member(name: &str, kind: MemberKind, shape: Shape) -> Member<String> {
        Member::new(name, name, kind, shape)
    }

    #[test]
    fn resolves_like_the_runtime() {
        let mut schema = TestSchema::default();
        schema.context.members.push(member("Title", MemberKind::Field, Shape::Text));
        schema.context.members.push(member("prefs", MemberKind::Field, Shape::Object));
        let mut customers = Member::new("customers", "customers", MemberKind::Component, Shape::List);
        customers.children.push(Member::new("Name", "customers.Name", MemberKind::Column, Shape::Text));
        schema.components.push(customers);
        assert!(matches!(schema.resolve("Title"), Resolution::Found(m) if m.name == "Title"));
        assert!(matches!(schema.resolve("customers.Current.Name"), Resolution::Found(m) if m.path == "customers.Name"));
        assert_eq!(schema.resolve("customers.Missing"), Resolution::Missing);
        assert_eq!(schema.resolve("prefs.font"), Resolution::Unknown);
        assert_eq!(schema.resolve("Nothing"), Resolution::Missing);
        schema.context.open = true;
        assert_eq!(schema.resolve("Nothing"), Resolution::Unknown);
        assert_eq!(schema.all_members().len(), 4);
    }

    #[test]
    fn shapes_of_rust_types_and_json() {
        assert_eq!(Shape::of_rust("Option<String>"), Shape::Text);
        assert_eq!(Shape::of_rust("Vec<Row>"), Shape::List);
        assert_eq!(Shape::of_rust("&'static str"), Shape::Text);
        assert_eq!(Shape::of_value_variant("F32"), Shape::Number);
        assert_eq!(Shape::of_json(&serde_json::json!(true)), Shape::Bool);
        assert_eq!(Shape::of_name("Object"), Shape::Object);
    }

    #[test]
    fn serializes_in_camel_case_without_empty_members() {
        let m = member("Title", MemberKind::RowField, Shape::Text);
        let v = serde_json::to_value(&m).expect("serializes");
        assert_eq!(v["kind"], "rowField");
        assert_eq!(v["expression"], "{Binding Title}");
        assert!(v.get("location").is_none() && v.get("children").is_none() && v.get("doc").is_none());
    }
}
