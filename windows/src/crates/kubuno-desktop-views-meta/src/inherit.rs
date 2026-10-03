//! Visual inheritance of views (`vskubuno/docs/EVENTS.md`, "User controls", Windows Forms' inherited forms and
//! inherited user controls): a view whose root names a base view, `x:Inherits="base_form.kbview"` (relative to the
//! view's file), shows every control of the base view plus its own, and may change the properties of the base's
//! controls whose `Modifiers` is `Protected` or `Public` — never those of a private one, which stays locked.
//!
//! The derived view writes:
//!
//! - its root, whose attributes override the base root's (`Title`, `DesignWidth`…);
//! - an element with the `x:Name` of a base control, *at the same place* (inside the elements that override the
//!   base control's containers): an override, whose attributes override the base control's and whose children come
//!   before the base control's own;
//! - any other element: a control of its own.
//!
//! [`merge`] produces the view the program compiles (and the designer renders): the derived elements first, in their
//! order — so an element of the derived file keeps its place (its designer id) —, then the base controls it does not
//! override, marked `x:Inherited="true"` (their descendants `x:Inherited="inner"`); an override is marked
//! `x:Inherited="override"`. Dependency-free: the `#[kubuno_desktop::view]` and `#[derive(UserControl)]` macros embed the
//! merged view, the runtime and the design surface merge again on a hot reload or a designer edit.

use crate::kbview::{scan, KbElement};

/// The attribute of a view's root naming its base view.
pub const INHERITS_ATTRIBUTE: &str = "x:Inherits";

/// The attribute [`merge`] puts on the elements coming from the base view.
pub const INHERITED_ATTRIBUTE: &str = "x:Inherited";

/// The deepest chain of inherited views (a view inheriting itself through others would loop).
pub const MAX_DEPTH: usize = 8;

/// An element of a view, with its children.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Node {
    name: String,
    attributes: Vec<(String, String)>,
    children: Vec<Node>,
    line: usize,
}

impl Node {
    fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
    }

    fn set(&mut self, name: &str, value: &str) {
        match self.attributes.iter_mut().find(|(n, _)| n == name) {
            Some((_, v)) => *v = value.to_string(),
            None => self.attributes.push((name.to_string(), value.to_string())),
        }
    }

    fn remove(&mut self, name: &str) {
        self.attributes.retain(|(n, _)| n != name);
    }

    fn x_name(&self) -> Option<&str> {
        self.attribute("x:Name")
    }

    /// Whether a derived view may change it (`Modifiers="Protected"` or `"Public"`).
    fn overridable(&self) -> bool {
        matches!(self.attribute("Modifiers"), Some("Protected") | Some("Public") | Some("ProtectedInternal") | Some("Internal"))
    }

    fn find(&self, name: &str) -> bool {
        self.x_name() == Some(name) || self.children.iter().any(|c| c.find(name))
    }

    /// Marks it (and its descendants) as coming from the base view.
    fn mark_inherited(&mut self, top: bool) {
        self.set(INHERITED_ATTRIBUTE, if top { "true" } else { "inner" });
        for c in &mut self.children {
            c.mark_inherited(false);
        }
    }
}

fn tree(elements: &[KbElement]) -> Option<Node> {
    fn build(elements: &[KbElement], i: &mut usize) -> Node {
        let e = &elements[*i];
        let depth = e.depth;
        let mut node = Node { name: e.name.clone(), attributes: e.attributes.clone(), children: Vec::new(), line: e.line };
        *i += 1;
        while *i < elements.len() && elements[*i].depth > depth {
            node.children.push(build(elements, i));
        }
        node
    }
    if elements.is_empty() {
        return None;
    }
    let mut i = 0;
    Some(build(elements, &mut i))
}

fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '"' => out.push_str("&quot;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
    out
}

fn write(node: &Node, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    out.push_str(&indent);
    out.push('<');
    out.push_str(&node.name);
    for (n, v) in &node.attributes {
        out.push(' ');
        out.push_str(n);
        out.push_str("=\"");
        out.push_str(&escape(v));
        out.push('"');
    }
    if node.children.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push_str(">\n");
    for c in &node.children {
        write(c, depth + 1, out);
    }
    out.push_str(&indent);
    out.push_str("</");
    out.push_str(&node.name);
    out.push_str(">\n");
}

/// The prefix of a design-time attribute (`d:Text="Alice"`, `d:Visible="false"`, `d:ItemsSource="sample.json"`).
pub const DESIGN_PREFIX: &str = "d:";

/// The view as the designer shows it (XAML's `d:` namespace): every `d:X="…"` replaces `X` (a bound `Visible`
/// becomes `false`, a bound `Text` a sample). `d:Visible` stays as well (the designer hides a `d:Visible="false"`
/// control, see `kubuno_desktop_views::common`), and `d:ItemsSource` stays as written (the `<Repeater>` reads its sample
/// rows from it). `None` when the view has no design-time attribute (nothing to rewrite).
pub fn apply_design_attributes(text: &str) -> Option<String> {
    let elements = scan(text).ok()?;
    if !elements.iter().any(|e| e.attributes.iter().any(|(n, _)| n.starts_with(DESIGN_PREFIX) && n != "d:ItemsSource")) {
        return None;
    }
    fn apply(node: &mut Node) {
        let design: Vec<(String, String)> = node
            .attributes
            .iter()
            .filter_map(|(n, v)| n.strip_prefix(DESIGN_PREFIX).filter(|p| !p.is_empty() && *p != "ItemsSource").map(|p| (p.to_string(), v.clone())))
            .collect();
        for (name, value) in design {
            node.set(&name, &value);
            // `d:Visible` stays too: the designer shows every control whatever its `Visible`, except one
            // that `d:Visible="false"` hides there.
            if name != "Visible" {
                node.remove(&format!("{DESIGN_PREFIX}{name}"));
            }
        }
        for c in &mut node.children {
            apply(c);
        }
    }
    let mut root = tree(&elements)?;
    apply(&mut root);
    let mut out = String::new();
    write(&root, 0, &mut out);
    Some(out)
}

/// The base view a view's root names (`x:Inherits`), if any.
pub fn inherits(text: &str) -> Option<String> {
    let elements = scan(text).ok()?;
    let root = elements.first()?;
    root.attribute(INHERITS_ATTRIBUTE).map(str::trim).filter(|v| !v.is_empty()).map(str::to_string)
}

/// The children of a derived element (`derived`) merged over those of the base element it stands for (`base`):
/// the derived ones first (overrides merged), then the base ones it did not override, marked inherited.
fn merge_children(derived: &[Node], base: &[Node], base_root: &Node) -> Result<Vec<Node>, String> {
    let mut consumed = vec![false; base.len()];
    let mut out = Vec::new();
    for d in derived {
        let overridden = d.x_name().and_then(|name| base.iter().position(|b| b.x_name() == Some(name)));
        match overridden {
            Some(index) => {
                let b = &base[index];
                let name = b.x_name().unwrap_or_default();
                // A private container written only to reach its Protected/Public children (`<Panel x:Name="body">…`,
                // nothing changed on it): it stays the base's, locked.
                if !b.overridable() && d.attributes.iter().all(|(n, _)| n == "x:Name") {
                    consumed[index] = true;
                    let mut kept = b.clone();
                    kept.set(INHERITED_ATTRIBUTE, "true");
                    kept.children = merge_children(&d.children, &b.children, base_root)?;
                    out.push(kept);
                    continue;
                }
                if !b.overridable() {
                    return Err(format!(
                        "line {}: `{name}` is a private control of the base view: set its Modifiers to Protected or Public there to change it here",
                        d.line
                    ));
                }
                if d.name != b.name {
                    return Err(format!("line {}: `{name}` is a <{}> in the base view, not a <{}>", d.line, b.name, d.name));
                }
                consumed[index] = true;
                let mut merged = b.clone();
                for (n, v) in &d.attributes {
                    merged.set(n, v);
                }
                merged.set(INHERITED_ATTRIBUTE, "override");
                merged.children = merge_children(&d.children, &b.children, base_root)?;
                out.push(merged);
            }
            None => {
                if let Some(name) = d.x_name().filter(|n| base_root.find(n)) {
                    return Err(format!(
                        "line {}: `{name}` is a control of the base view: override it at the same place (inside the elements that stand for its containers)",
                        d.line
                    ));
                }
                let mut own = d.clone();
                own.children = merge_children(&d.children, &[], base_root)?;
                out.push(own);
            }
        }
    }
    for (b, used) in base.iter().zip(consumed) {
        if !used {
            let mut inherited = b.clone();
            inherited.mark_inherited(true);
            out.push(inherited);
        }
    }
    Ok(out)
}

/// The view `derived_text` merged over its base view `base_text` (see the module doc).
pub fn merge(base_text: &str, derived_text: &str) -> Result<String, String> {
    let base = tree(&scan(base_text).map_err(|e| format!("the base view, {e}"))?).ok_or("the base view has no root element")?;
    let derived = tree(&scan(derived_text).map_err(|e| e.to_string())?).ok_or("the view has no root element")?;
    let mut root = Node { name: derived.name.clone(), attributes: base.attributes.clone(), children: Vec::new(), line: derived.line };
    // The base's own handlers and class stay its own: the derived root names its own.
    root.remove("x:Class");
    for (n, v) in &derived.attributes {
        if n != INHERITS_ATTRIBUTE {
            root.set(n, v);
        }
    }
    root.set(INHERITED_ATTRIBUTE, "root");
    root.children = merge_children(&derived.children, &base.children, &base)?;
    let mut out = String::new();
    write(&root, 0, &mut out);
    Ok(out)
}

/// The view `text` (the file `file`, for its relative base path) with its whole chain of base views merged in,
/// reading each base with `read`; `Ok(None)` when it inherits nothing. Also returns the base files read (what a
/// macro tracks).
pub fn resolve(text: &str, file: Option<&std::path::Path>, read: &dyn Fn(&std::path::Path) -> std::io::Result<String>) -> Result<Option<(String, Vec<std::path::PathBuf>)>, String> {
    let Some(base) = inherits(text) else { return Ok(None) };
    let mut files = Vec::new();
    let merged = resolve_chain(text, &base, file, read, &mut files, 0)?;
    Ok(Some((merged, files)))
}

fn resolve_chain(
    text: &str,
    base: &str,
    file: Option<&std::path::Path>,
    read: &dyn Fn(&std::path::Path) -> std::io::Result<String>,
    files: &mut Vec<std::path::PathBuf>,
    depth: usize,
) -> Result<String, String> {
    if depth >= MAX_DEPTH {
        return Err(format!("more than {MAX_DEPTH} views inherit from one another (does a view inherit from itself?)"));
    }
    let dir = file.and_then(std::path::Path::parent).unwrap_or(std::path::Path::new(""));
    let base_path = dir.join(base);
    let base_text = read(&base_path).map_err(|e| format!("cannot read the base view `{base}` ({}): {e}", base_path.display()))?;
    files.push(base_path.clone());
    // The base may inherit too: merge its own base first.
    let base_text = match inherits(&base_text) {
        Some(grand) => resolve_chain(&base_text, &grand, Some(&base_path), read, files, depth + 1)?,
        None => base_text,
    };
    merge(&base_text, text)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = r#"<!-- The base form. -->
<Panel x:Class="BaseForm" DesignWidth="400" DesignHeight="300" Title="Base" OnLoad="base_load">
  <Label x:Name="header" Text="Base header" X="8" Y="8" Width="200" Height="24"/>
  <Button x:Name="ok" Modifiers="Protected" Text="OK" X="300" Y="260" Width="90" Height="32" OnClick="ok_click"/>
  <Panel x:Name="body" Modifiers="Public" X="8" Y="40" Width="384" Height="200">
    <TextField x:Name="name" Modifiers="Protected" X="0" Y="0" Width="200" Height="32"/>
  </Panel>
</Panel>"#;

    #[test]
    fn a_derived_view_keeps_its_elements_first_and_adds_the_base_controls() {
        let derived = r#"<Panel x:Class="Derived" x:Inherits="base.kbview" Title="Derived">
  <Button x:Name="ok" Text="Send"/>
  <Panel x:Name="body"><TextField x:Name="name" Width="300"/></Panel>
  <CheckBox x:Name="remember" Text="Remember" X="8" Y="260"/>
</Panel>"#;
        let merged = merge(BASE, derived).unwrap();
        let elements = scan(&merged).unwrap();
        let root = &elements[0];
        assert_eq!((root.attribute("Title"), root.attribute("DesignWidth"), root.attribute("OnLoad"), root.attribute("x:Inherits")), (Some("Derived"), Some("400"), Some("base_load"), None));
        let names: Vec<_> = elements.iter().skip(1).map(|e| (e.x_name().unwrap_or(""), e.depth, e.attribute(INHERITED_ATTRIBUTE).unwrap_or(""))).collect();
        assert_eq!(
            names,
            [("ok", 1, "override"), ("body", 1, "override"), ("name", 2, "override"), ("remember", 1, ""), ("header", 1, "true")],
            "the derived elements keep their places, the base controls follow"
        );
        let ok = &elements[1];
        assert_eq!((ok.attribute("Text"), ok.attribute("X"), ok.attribute("OnClick")), (Some("Send"), Some("300"), Some("ok_click")));
        assert_eq!(elements[3].attribute("Width"), Some("300"));
    }

    #[test]
    fn private_controls_cannot_be_changed_and_overrides_stay_in_place() {
        let err = merge(BASE, r#"<Panel x:Inherits="b"><Label x:Name="header" Text="x"/></Panel>"#).unwrap_err();
        assert!(err.contains("private control") && err.contains("Modifiers"), "{err}");
        let err = merge(BASE, r#"<Panel x:Inherits="b"><TextField x:Name="name" Text="x"/></Panel>"#).unwrap_err();
        assert!(err.contains("same place"), "{err}");
        let err = merge(BASE, r#"<Panel x:Inherits="b"><Label x:Name="ok"/></Panel>"#).unwrap_err();
        assert!(err.contains("is a <Button>"), "{err}");
    }

    #[test]
    fn chains_are_resolved_relative_to_each_file_and_cycles_stop() {
        let read = |p: &std::path::Path| -> std::io::Result<String> {
            match p.file_name().and_then(|n| n.to_str()) {
                Some("base.kbview") => Ok(BASE.to_string()),
                Some("middle.kbview") => Ok(r#"<Panel x:Inherits="base.kbview" Title="Middle"><Button x:Name="extra" Modifiers="Public" Text="Extra"/></Panel>"#.to_string()),
                Some("loop.kbview") => Ok(r#"<Panel x:Inherits="loop.kbview"/>"#.to_string()),
                _ => Err(std::io::Error::new(std::io::ErrorKind::NotFound, "missing")),
            }
        };
        let (merged, files) = resolve(r#"<Panel x:Inherits="middle.kbview"><Button x:Name="extra" Text="Changed"/></Panel>"#, Some(std::path::Path::new("C:/app/src/derived.kbview")), &read)
            .unwrap()
            .unwrap();
        assert_eq!(files.iter().map(|f| f.file_name().unwrap().to_string_lossy().into_owned()).collect::<Vec<_>>(), ["middle.kbview", "base.kbview"]);
        assert!(merged.contains("Text=\"Changed\"") && merged.contains("x:Name=\"header\"") && merged.contains("Title=\"Middle\""), "{merged}");
        assert!(resolve("<Panel x:Inherits=\"loop.kbview\"/>", None, &read).unwrap_err().contains("inherit from one another"));
        assert!(resolve("<Panel x:Inherits=\"nope.kbview\"/>", None, &read).unwrap_err().contains("cannot read"));
        assert_eq!(resolve("<Panel/>", None, &read).unwrap(), None);
    }

    #[test]
    fn design_attributes_replace_their_run_time_ones() {
        let view = r#"<Panel><EmptyState Visible="{Binding NoConversation}" d:Visible="false"/><Label Text="{Binding Name}" d:Text="Alice"/><Repeater d:ItemsSource="sample.json"/></Panel>"#;
        let designed = apply_design_attributes(view).unwrap();
        let e = scan(&designed).unwrap();
        assert_eq!((e[1].attribute("Visible"), e[1].attribute("d:Visible")), (Some("false"), Some("false")), "kept for the designer");
        assert_eq!(e[2].attribute("Text"), Some("Alice"));
        assert_eq!(e[3].attribute("d:ItemsSource"), Some("sample.json"), "kept for the Repeater");
        assert_eq!(apply_design_attributes("<Panel><Label Text=\"x\"/></Panel>"), None);
    }

    #[test]
    fn values_round_trip_through_the_merge() {
        let merged = merge(r#"<Panel><Label x:Name="l" Modifiers="Public" Text="a &amp; b&#10;c"/></Panel>"#, r#"<Panel x:Inherits="b"/>"#).unwrap();
        assert_eq!(scan(&merged).unwrap()[1].attribute("Text"), Some("a & b\nc"));
    }
}
