//! Surgical edits — `XML_VIEWS.md`'s phase 2a brief: "set/insert/remove an
//! attribute, insert/remove/move a child element — each preserving all
//! untouched bytes … and producing minimal diffs".
//!
//! ## Implementation choice
//!
//! Every operation here computes one or more precise byte ranges on the
//! *original* source text (found through the parsed tree's
//! [`rowan::TextRange`]s) and replaces each — it does not mutate `rowan`'s
//! green tree in place (via `GreenNode::replace_child`/friends). Both
//! approaches satisfy the actual requirement ("preserve untouched bytes",
//! "minimal diff", "exact output text" in tests); a text splice is the one
//! that does not require guessing at green-tree mutation internals the
//! crate's public docs do not spell out in enough detail to get right blind,
//! and it is trivially reasoned about and tested (every function below is
//! "compute a range, replace it"). A caller that needs the result as a tree
//! again just re-parses it — [`crate::syntax::parse`] is cheap (§5's own
//! "parse+compile once per file change", not per frame) and re-parsing after
//! an edit is exactly the workflow a designer or Claude already has (edit →
//! show the new file → parse again for the next edit). A future phase can
//! switch the *internals* of these functions to true green-tree splicing
//! without changing this public API, if incremental re-parsing ever becomes
//! the bottleneck it is not at this scale (`.kbview` files are hand-sized UI
//! descriptions, not generated megabyte documents).
//!
//! Every function takes the AST node(s) to operate on (from a tree already
//! parsed with [`crate::syntax::parse`]) and returns the precise [`Edit`]s to
//! apply against that *same, original* text — not a whole new document
//! string. This is what `vskubuno/docs/DESIGNER.md`'s "DSG-2 protocol"
//! section needs: the VS side applies each `{range, newText}` pair as one
//! minimal `ITextEdit.Replace`, instead of diffing two whole-file strings to
//! find what changed. [`apply_edits`] is the one place that still produces a
//! whole-file string, for a caller (this module's own tests, or anything that
//! wants to re-parse in one step) that wants that instead.
//!
//! A single designer gesture can need more than one disjoint range (moving an
//! element touches both where it used to be and where it now is; renaming
//! touches both the start and end tag when they are separate) — every
//! function therefore returns `Vec<Edit>`, empty for a no-op (an out-of-range
//! index, a missing attribute to remove, an id that does not resolve to
//! anything). Every `Edit` a single call returns is disjoint from every
//! other, and each is expressed in the *original* document's byte offsets, so
//! applying them in any order (front-to-back or back-to-front) reproduces the
//! same result — [`apply_edits`] applies them back-to-front for exactly that
//! reason.

use crate::ast::{AstNode, Attribute, Element};
use crate::syntax::{SyntaxElement, SyntaxKind, SyntaxNode};
use rowan::TextRange;

/// One minimal, precise text replacement against the *original* source:
/// "swap `range` for `new_text`". See the module doc for why every edit
/// operation returns a `Vec` of these rather than a whole new document
/// string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub range: TextRange,
    pub new_text: String,
}

/// Applies `edits` (as produced by this module's functions, always disjoint
/// and in `text`'s own original coordinates) to `text`, returning the whole
/// document's new text — the single value the pre-"DSG-2 protocol" API used
/// to return directly. Edits are applied back-to-front (by descending
/// `range.start()`) so that splicing one never invalidates another's
/// still-to-be-applied byte offsets.
pub fn apply_edits(text: &str, edits: &[Edit]) -> String {
    let mut ordered: Vec<&Edit> = edits.iter().collect();
    ordered.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
    let mut out = text.to_string();
    for edit in ordered {
        out = splice(&out, edit.range, &edit.new_text);
    }
    out
}

/// The full source text a node's tree was parsed from, reconstructed by
/// walking to the root ([`SyntaxKind::DOCUMENT`]) and reading it back —
/// exact because the tree is lossless (`XML_VIEWS.md` §6). Currently unused
/// by this module's own edit functions (each computes a range against the
/// caller-supplied text directly), kept for a caller that only has a node
/// and needs its whole document back without threading the original string
/// through separately.
#[allow(dead_code)]
fn root_text(node: &SyntaxNode) -> String {
    // `ancestors()` always yields at least `node` itself, so this fallback
    // never actually triggers — kept instead of `.expect()` so there is no
    // panic path in production code at all (`CLAUDE.md` §7's "no `unwrap`
    // outside tests, `expect` only at bootstrap").
    node.ancestors().last().unwrap_or_else(|| node.clone()).text().to_string()
}

fn splice(text: &str, range: TextRange, replacement: &str) -> String {
    let start = usize::from(range.start());
    let end = usize::from(range.end());
    let mut out = String::with_capacity(text.len() - (end - start) + replacement.len());
    out.push_str(&text[..start]);
    out.push_str(replacement);
    out.push_str(&text[end..]);
    out
}

/// Escapes `value` so it can sit inside an attribute delimited by `quote` and reads back unchanged
/// through [`crate::ast::Attribute::value`] (which decodes character references): `&`, `<`, the
/// quote character, and line breaks and tabs (which an XML reader would otherwise turn into spaces).
fn escape_attribute_value(value: &str, quote: char) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            '\t' => out.push_str("&#9;"),
            c if c == quote => out.push_str(if quote == '"' { "&quot;" } else { "&apos;" }),
            c => out.push(c),
        }
    }
    out
}

// ── attributes ──────────────────────────────────────────────────────────

/// Sets `name="value"` on `element`, creating the attribute if absent.
///
/// - If the attribute exists, only its value's bytes (between the quotes)
///   change — the quote style, the attribute's position, and every other
///   attribute are untouched.
/// - If it does not, `<Name a="1"/>` gains it right before the tag's closer
///   (`<Name a="1" name="value"/>`), double-quoted.
pub fn set_attribute(element: &Element, name: &str, value: &str) -> Vec<Edit> {
    if let Some(attr) = element.attribute(name) {
        if let Some(range) = attr.value_range() {
            let quote = quote_char_of(&attr).unwrap_or('"');
            return vec![Edit { range, new_text: escape_attribute_value(value, quote) }];
        }
    }

    // Not present (or malformed beyond repair) — insert a brand new
    // `<space>name="value"` right before the start tag's closer.
    let Some(start_tag) = element.start_tag() else { return Vec::new() };
    let Some(closer) = tag_closer(&start_tag) else { return Vec::new() };
    let insertion = format!(" {name}=\"{}\"", escape_attribute_value(value, '"'));
    vec![Edit { range: TextRange::empty(closer.text_range().start()), new_text: insertion }]
}

/// Removes `name` from `element`, along with the single run of whitespace
/// immediately before it (so no double space is left behind). No edit at all
/// when the attribute is not present.
pub fn remove_attribute(element: &Element, name: &str) -> Vec<Edit> {
    let Some(attr) = element.attribute(name) else { return Vec::new() };
    let Some(start_tag) = element.start_tag() else { return Vec::new() };
    let range = delete_range_absorbing_leading_whitespace(&start_tag, attr.syntax());
    vec![Edit { range, new_text: String::new() }]
}

fn quote_char_of(attr: &Attribute) -> Option<char> {
    attr.raw_value()?.chars().next()
}

/// The `>` or `/>` token that closes a start tag.
fn tag_closer(start_tag: &SyntaxNode) -> Option<crate::syntax::SyntaxToken> {
    start_tag
        .children_with_tokens()
        .filter_map(|e| e.into_token())
        .find(|t| matches!(t.kind(), SyntaxKind::R_ANGLE | SyntaxKind::SLASH_R_ANGLE))
}

// ── child elements ──────────────────────────────────────────────────────

/// Inserts `child_xml` (a well-formed `.kbview` fragment, e.g.
/// `"<Label Text=\"hi\"/>"`) as `parent`'s child at `index` (0-based, among
/// element children only — text/comments do not count), shifting the
/// existing child currently at `index` (and everything after it) later. An
/// `index` at or past the current child count appends at the end.
///
/// `child_xml` is inserted byte-for-byte with no added whitespace — this
/// function is a precise splice primitive, not a formatter; a caller that
/// wants the new child on its own indented line includes that formatting in
/// `child_xml` itself (see the module doc's "implementation choice").
///
/// A self-closing `<Parent/>` is turned into `<Parent>…</Parent>` first, so
/// inserting into an empty container works.
pub fn insert_child(parent: &Element, index: usize, child_xml: &str) -> Vec<Edit> {
    if parent.is_self_closing() {
        let Some(start_tag) = parent.start_tag() else { return Vec::new() };
        let Some(closer) = tag_closer(&start_tag) else { return Vec::new() };
        if closer.kind() != SyntaxKind::SLASH_R_ANGLE {
            return Vec::new(); // Malformed tree — nothing sane to splice.
        }
        let name = parent.name().unwrap_or_default();
        let replacement = format!(">{child_xml}</{name}>");
        return vec![Edit { range: closer.text_range(), new_text: replacement }];
    }

    let children: Vec<Element> = parent.children().collect();
    let offset = if index < children.len() {
        children[index].syntax().text_range().start()
    } else {
        match parent.end_tag() {
            Some(end_tag) => end_tag.text_range().start(),
            None => return Vec::new(), // Malformed tree (no end tag despite not self-closing).
        }
    };
    vec![Edit { range: TextRange::empty(offset), new_text: child_xml.to_string() }]
}

/// Removes `parent`'s child element at `index`, along with the run of
/// whitespace immediately before it. No edit at all when `index` is out of
/// range.
pub fn remove_child(parent: &Element, index: usize) -> Vec<Edit> {
    let children: Vec<Element> = parent.children().collect();
    let Some(target) = children.get(index) else { return Vec::new() };
    let range = delete_range_absorbing_leading_whitespace(parent.syntax(), target.syntax());
    vec![Edit { range, new_text: String::new() }]
}

/// Moves `parent`'s child element from `from` to `to` (`Vec::remove` +
/// `Vec::insert` semantics: `to` is the index in the list *after* the child
/// has been removed, clamped to the new length). No edit at all when
/// `from`/`to` are out of range or equal.
///
/// The moved element keeps its own line layout: the whitespace that preceded
/// it (typically a newline plus its indentation) is removed with it and
/// re-emitted at the new slot, so a one-element-per-line container stays one
/// element per line (`<Stack>\n  <A/>\n  <B/>\n</Stack>` → `<Stack>\n  <B/>\n
/// <A/>\n</Stack>` for "bring `A` to the front"). An element written on the
/// same line as its siblings, with no whitespace before it, moves as its bare
/// tag-to-tag text. Returns two disjoint edits (delete the old slot, insert at
/// the new one); see the module doc for why that composition is safe
/// regardless of which one is textually first.
pub fn move_child(parent: &Element, from: usize, to: usize) -> Vec<Edit> {
    let children: Vec<Element> = parent.children().collect();
    if from == to || from >= children.len() {
        return Vec::new();
    }

    let moved = &children[from];
    let moved_text = moved.syntax().text().to_string();
    let delete_range = delete_range_absorbing_leading_whitespace(parent.syntax(), moved.syntax());
    // The whitespace the delete range absorbed (empty for an inline element).
    let leading = leading_whitespace(moved.syntax()).map(|t| t.text().to_string()).unwrap_or_default();

    // Every other child, in order, with `from` removed — exactly the list
    // `to` is an index into (`Vec::remove` + `Vec::insert`'s own contract).
    let remaining: Vec<&Element> = children.iter().enumerate().filter(|(i, _)| *i != from).map(|(_, e)| e).collect();
    let to = to.min(remaining.len());
    let (target_offset, new_text) = match remaining.get(to) {
        // Before a sibling: the moved text, then its own leading whitespace
        // (the sibling's own indentation, already in place, now precedes it).
        Some(e) => (e.syntax().text_range().start(), format!("{moved_text}{leading}")),
        // At the end, after the last remaining sibling, on its own line.
        None if !leading.is_empty() => match remaining.last() {
            Some(last) => (last.syntax().text_range().end(), format!("{leading}{moved_text}")),
            None => return Vec::new(),
        },
        None => match parent.end_tag() {
            Some(end_tag) => (end_tag.text_range().start(), moved_text),
            None => return Vec::new(),
        },
    };

    vec![
        Edit { range: delete_range, new_text: String::new() },
        Edit { range: TextRange::empty(target_offset), new_text },
    ]
}

/// The whitespace-only token directly before `node` among its siblings, if any.
fn leading_whitespace(node: &SyntaxNode) -> Option<crate::syntax::SyntaxToken> {
    node.prev_sibling_or_token()?.into_token().filter(|t| {
        matches!(t.kind(), SyntaxKind::WHITESPACE | SyntaxKind::TEXT) && t.text().chars().all(char::is_whitespace)
    })
}

/// Moves the child at `from` in `source_parent` to index `to` in
/// `dest_parent` — the general case [`move_child`] does not cover, for a
/// drag that drops an element into a *different* container (`XML_VIEWS.md`
/// §1's Dock/Flow bands, `DESIGNER.md` §4). Delegates to [`move_child`]
/// verbatim when `source_parent`/`dest_parent` are the same element (by
/// identity — same underlying [`SyntaxNode`]), for its exact, already-tested
/// same-parent behavior; otherwise removes the child from `source_parent`
/// and inserts its own `<Tag …>…</Tag>` text into `dest_parent`, which is
/// always safe as two disjoint edits since the two parents' subtrees never
/// overlap in that case.
///
/// No edit at all when `from` is out of range, or when `dest_parent` is the
/// moved element itself or one of its own descendants (moving a container
/// into its own contents has no sane result).
pub fn move_element(source_parent: &Element, from: usize, dest_parent: &Element, to: usize) -> Vec<Edit> {
    if source_parent.syntax() == dest_parent.syntax() {
        return move_child(source_parent, from, to);
    }

    let source_children: Vec<Element> = source_parent.children().collect();
    let Some(moved) = source_children.get(from) else { return Vec::new() };

    let moved_is_ancestor_of_dest =
        dest_parent.syntax() == moved.syntax() || dest_parent.syntax().ancestors().any(|a| &a == moved.syntax());
    if moved_is_ancestor_of_dest {
        return Vec::new();
    }

    let moved_text = moved.syntax().text().to_string();
    let mut edits = remove_child(source_parent, from);
    edits.extend(insert_child(dest_parent, to, &moved_text));
    edits
}

/// Reorders `parent`'s child elements: slot `i` receives the child currently at `order[i]` (so
/// `order` lists the current indices in their new order) - the designer's "Bring to Front"/"Send to
/// Back" on a multi-selection (`DESIGNER.md` §13), which moves several siblings at once. Only the
/// elements' own text moves: whatever sits between them (their indentation, comments) stays in
/// place, so a one-element-per-line container stays one element per line. One edit per slot whose
/// element changes, all disjoint. No edit at all when `order` is not a permutation of the children.
pub fn reorder_children(parent: &Element, order: &[usize]) -> Vec<Edit> {
    let children: Vec<Element> = parent.children().collect();
    if order.len() != children.len() {
        return Vec::new();
    }
    let mut seen = vec![false; children.len()];
    for &i in order {
        if i >= children.len() || std::mem::replace(&mut seen[i], true) {
            return Vec::new();
        }
    }
    order
        .iter()
        .enumerate()
        .filter(|(slot, from)| slot != *from)
        .map(|(slot, &from)| Edit {
            range: children[slot].syntax().text_range(),
            new_text: children[from].syntax().text().to_string(),
        })
        .collect()
}

/// Renames `element`'s tag — its start-tag name token, and its end-tag name
/// token too when it has one (a self-closing element only has the one to
/// change). Every attribute, and the element's `x:Name` (a *value*, never
/// the tag name), is left untouched.
pub fn rename_element(element: &Element, new_name: &str) -> Vec<Edit> {
    let mut edits = Vec::new();
    if let Some(range) = element.name_range() {
        edits.push(Edit { range, new_text: new_name.to_string() });
    }
    if let Some(range) = element.end_name_range() {
        edits.push(Edit { range, new_text: new_name.to_string() });
    }
    edits
}

// ── designer clipboard / structure gestures ─────────────────────────────

/// Every `x:Name` value used anywhere under (and including) `root`.
pub fn collect_names(root: &Element) -> std::collections::HashSet<String> {
    root.syntax()
        .descendants()
        .filter_map(Element::cast)
        .filter_map(|e| e.attribute("x:Name").and_then(|a| a.value()))
        .filter(|n| !n.is_empty())
        .collect()
}

/// A name not in `taken`, derived from `name` the way the WinForms designer names a pasted copy:
/// trailing digits are replaced by the first free number from 2 (`button1` → `button2`,
/// `status` → `status2`).
pub fn unique_name(name: &str, taken: &std::collections::HashSet<String>) -> String {
    if !taken.contains(name) {
        return name.to_string();
    }
    let base = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let base = if base.is_empty() { name } else { base };
    (2u32..)
        .map(|n| format!("{base}{n}"))
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or_else(|| name.to_string())
}

/// Inserts a copied `.kbview` fragment (one element, possibly with children, as produced by the
/// designer's Copy/Duplicate) as `parent`'s child at `index` — the Paste/Duplicate gesture.
/// Unlike [`insert_child`] (a verbatim splice), this one lays the fragment out like the rest of
/// the file: on its own line, indented like its new siblings (or one level under `parent` when
/// it has none), the fragment's own inner lines re-indented to match. Every `x:Name` in the
/// fragment that already exists in `taken` (see [`collect_names`]) — or twice in the fragment
/// itself — is renamed with [`unique_name`], so a paste never creates a duplicate name.
///
/// `fragment` may hold several elements one after the other (a multi-selection copied together,
/// `DESIGNER.md` §13): they are inserted in order at `index`, each on its own line.
///
/// No edit at all when `fragment` is not one or more well-formed elements.
pub fn insert_fragment(
    parent: &Element,
    index: usize,
    fragment: &str,
    taken: &std::collections::HashSet<String>,
) -> Vec<Edit> {
    let Some(fragments) = renamed_fragments(fragment, taken) else { return Vec::new() };
    let text = document_text(parent.syntax());
    let nl = newline_of(&text);
    let parent_indent = line_indent(&text, parent.syntax()).unwrap_or_default();
    let children: Vec<Element> = parent.children().collect();
    let child_indent = children
        .iter()
        .find_map(|c| line_indent(&text, c.syntax()))
        .unwrap_or_else(|| format!("{parent_indent}{}", indent_unit(&text, parent)));
    let body = fragments
        .iter()
        .map(|f| reindent(&dedent_tail(f), &child_indent, nl))
        .collect::<Vec<_>>()
        .join(&format!("{nl}{child_indent}"));
    let inline_body: String = fragments.iter().map(|f| reindent(&dedent_tail(f), &child_indent, nl)).collect();

    if children.is_empty() {
        let Some(start_tag) = parent.start_tag() else { return Vec::new() };
        let Some(closer) = tag_closer(&start_tag) else { return Vec::new() };
        let block = format!("{nl}{child_indent}{body}{nl}{parent_indent}");
        if parent.is_self_closing() {
            if closer.kind() != SyntaxKind::SLASH_R_ANGLE {
                return Vec::new();
            }
            let name = parent.name().unwrap_or_default();
            return vec![Edit { range: closer.text_range(), new_text: format!(">{block}</{name}>") }];
        }
        // `<P></P>` or `<P>\n  </P>`: replace whatever whitespace sits between the two tags.
        let Some(end_tag) = parent.end_tag() else { return Vec::new() };
        let range = TextRange::new(closer.text_range().end(), end_tag.text_range().start());
        return vec![Edit { range, new_text: block }];
    }

    match children.get(index) {
        Some(sibling) if line_indent(&text, sibling.syntax()).is_some() => vec![Edit {
            range: TextRange::empty(sibling.syntax().text_range().start()),
            new_text: format!("{body}{nl}{child_indent}"),
        }],
        Some(sibling) => vec![Edit { range: TextRange::empty(sibling.syntax().text_range().start()), new_text: inline_body }],
        None => {
            let last = &children[children.len() - 1];
            let new_text = if line_indent(&text, last.syntax()).is_some() {
                format!("{nl}{child_indent}{body}")
            } else {
                body
            };
            vec![Edit { range: TextRange::empty(last.syntax().text_range().end()), new_text }]
        }
    }
}

/// Wraps `element` in a new `<wrapper>` container — the designer's "Wrap in" gesture. The element
/// moves one indentation level down, between the new tags, each on its own line; an element that
/// shares its line with other content is wrapped inline instead. No edit for a `wrapper` that is
/// not a valid element name.
pub fn wrap_element(element: &Element, wrapper: &str) -> Vec<Edit> {
    if !is_element_name(wrapper) {
        return Vec::new();
    }
    let text = document_text(element.syntax());
    let own = element.syntax().text().to_string();
    let new_text = match line_indent(&text, element.syntax()) {
        Some(indent) => {
            let nl = newline_of(&text);
            let unit = indent_unit(&text, element);
            let inner = reindent(&dedent_tail(&own), &format!("{indent}{unit}"), nl);
            format!("<{wrapper}>{nl}{indent}{unit}{inner}{nl}{indent}</{wrapper}>")
        }
        None => format!("<{wrapper}>{own}</{wrapper}>"),
    };
    vec![Edit { range: element.syntax().text_range(), new_text }]
}

/// Replaces the container `element` by its own children — the designer's "Remove container"
/// gesture (the children, and anything between them such as comments, move one indentation level
/// up). No edit for an element without children, or for the document root unless it has exactly
/// one child (a view always has a single root element).
pub fn unwrap_element(element: &Element) -> Vec<Edit> {
    let children: Vec<Element> = element.children().collect();
    let (Some(first), Some(last)) = (children.first(), children.last()) else { return Vec::new() };
    let is_root = element.syntax().parent().is_none_or(|p| p.kind() != SyntaxKind::ELEMENT);
    if is_root && children.len() != 1 {
        return Vec::new();
    }
    let text = document_text(element.syntax());
    let start = usize::from(first.syntax().text_range().start());
    let end = usize::from(last.syntax().text_range().end());
    let inner = &text[start..end];
    let new_text = match (line_indent(&text, element.syntax()), line_indent(&text, first.syntax())) {
        (Some(outer), Some(child)) if child.len() > outer.len() && child.starts_with(&outer) => {
            let nl = newline_of(&text);
            reindent(&dedent_tail(inner), &outer, nl)
        }
        _ => inner.to_string(),
    };
    vec![Edit { range: element.syntax().text_range(), new_text }]
}

/// `fragment` split into its top-level elements (one or more, `DESIGNER.md` §13), each with its
/// colliding `x:Name`s renamed - against `taken` and against the names the earlier elements of the
/// same fragment already use. `None` unless the fragment is a sequence of well-formed elements.
fn renamed_fragments(fragment: &str, taken: &std::collections::HashSet<String>) -> Option<Vec<String>> {
    // Parsed inside a wrapper so a sequence of elements is one well-formed document.
    let wrapped = format!("<Fragment>{}</Fragment>", fragment.trim());
    let parse = crate::syntax::parse(&wrapped);
    if !parse.diagnostics.is_empty() {
        return None;
    }
    let root = crate::ast::Document::cast(parse.syntax())?.root_element()?;
    let elements: Vec<String> = root.children().map(|e| e.syntax().text().to_string()).collect();
    if elements.is_empty() {
        return None;
    }
    let mut used = taken.clone();
    elements.iter().map(|e| renamed_fragment(e, &mut used)).collect()
}

/// `fragment` parsed as exactly one element, with its colliding `x:Name`s renamed (and every name it
/// ends up using added to `used`).
fn renamed_fragment(fragment: &str, used: &mut std::collections::HashSet<String>) -> Option<String> {
    let fragment = fragment.trim();
    let parse = crate::syntax::parse(fragment);
    if !parse.diagnostics.is_empty() {
        return None;
    }
    let doc = crate::ast::Document::cast(parse.syntax())?;
    let root = doc.root_element()?;
    // Nothing but the one element (comments/whitespace around it are fine).
    if parse.syntax().children().filter_map(Element::cast).count() != 1 {
        return None;
    }
    let mut edits = Vec::new();
    for element in root.syntax().descendants().filter_map(Element::cast) {
        let Some(name) = element.attribute("x:Name").and_then(|a| a.value()) else { continue };
        let fresh = unique_name(&name, used);
        if fresh != name {
            edits.extend(set_attribute(&element, "x:Name", &fresh));
        }
        used.insert(fresh);
    }
    let start = usize::from(root.syntax().text_range().start());
    let end = usize::from(root.syntax().text_range().end());
    let renamed = apply_edits(fragment, &edits);
    // Renaming only changes attribute values, so the root still starts at `start`; its end moved
    // by the total length delta.
    let delta = renamed.len() as isize - fragment.len() as isize;
    let end = (end as isize + delta) as usize;
    renamed.get(start..end).map(str::to_string)
}

fn is_element_name(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some(c) if c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c))
}

/// The whole document text `node` belongs to.
fn document_text(node: &SyntaxNode) -> String {
    node.ancestors().last().unwrap_or_else(|| node.clone()).text().to_string()
}

/// `new_text` with its line breaks written the way `document` writes them (CRLF when the document
/// uses CRLF, LF otherwise), so an edit never leaves the file with mixed line endings (a fragment
/// the host built with `\n` dropped into a CRLF view made Visual Studio ask to normalise them).
pub fn match_line_endings(document: &str, new_text: String) -> String {
    if !new_text.contains('\n') {
        return new_text;
    }
    let lf = new_text.replace("\r\n", "\n");
    if newline_of(document) == "\r\n" {
        lf.replace('\n', "\r\n")
    } else {
        lf
    }
}

fn newline_of(text: &str) -> &'static str {
    if text.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

/// The indentation of the line `node` starts on, when `node` is the first thing on that line;
/// `None` when other content precedes it on the same line.
fn line_indent(text: &str, node: &SyntaxNode) -> Option<String> {
    let start = usize::from(node.text_range().start());
    let line_start = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let prefix = &text[line_start..start];
    prefix.chars().all(|c| c == ' ' || c == '\t').then(|| prefix.to_string())
}

/// One indentation level as the file already uses it: the extra indentation of `near`'s first
/// child (or, failing that, of the first nested element anywhere in the file) over its parent,
/// else two spaces.
fn indent_unit(text: &str, near: &Element) -> String {
    let root = near.syntax().ancestors().last().unwrap_or_else(|| near.syntax().clone());
    let candidates = std::iter::once(near.syntax().clone()).chain(root.descendants());
    for node in candidates.filter(|n| n.kind() == SyntaxKind::ELEMENT) {
        let Some(child) = node.children().find(|c| c.kind() == SyntaxKind::ELEMENT) else { continue };
        if let (Some(outer), Some(inner)) = (line_indent(text, &node), line_indent(text, &child)) {
            if inner.len() > outer.len() && inner.starts_with(&outer) {
                return inner[outer.len()..].to_string();
            }
        }
    }
    "  ".to_string()
}

/// `s` with the common indentation of its lines after the first removed (the first line is the
/// start of an element, never indented itself).
fn dedent_tail(s: &str) -> String {
    let mut lines = s.lines();
    let first = lines.next().unwrap_or_default();
    let rest: Vec<&str> = lines.collect();
    let common = rest
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);
    let mut out = first.to_string();
    for line in rest {
        out.push('\n');
        out.push_str(line.get(common..).unwrap_or_else(|| line.trim_start()));
    }
    out
}

/// `s` (a block whose lines after the first are relative to column 0) with every non-blank line
/// after the first prefixed by `indent`, joined with `nl`.
fn reindent(s: &str, indent: &str, nl: &str) -> String {
    let mut out = String::new();
    for (i, line) in s.lines().enumerate() {
        if i > 0 {
            out.push_str(nl);
            if !line.trim().is_empty() {
                out.push_str(indent);
            }
        }
        out.push_str(line.trim_end_matches('\r'));
    }
    out
}

/// `target`'s own range, extended to also cover the single whitespace-only
/// `TEXT` token immediately preceding it among `container`'s children (if
/// any) — the shared "remove this node cleanly" rule [`remove_attribute`],
/// [`remove_child`] and [`move_child`] all use.
fn delete_range_absorbing_leading_whitespace(container: &SyntaxNode, target: &SyntaxNode) -> TextRange {
    let mut previous: Option<SyntaxElement> = None;
    for el in container.children_with_tokens() {
        if el.as_node() == Some(target) {
            break;
        }
        previous = Some(el);
    }
    let end = target.text_range().end();
    let is_whitespace_text = |t: &crate::syntax::SyntaxToken| {
        matches!(t.kind(), SyntaxKind::WHITESPACE | SyntaxKind::TEXT) && t.text().chars().all(char::is_whitespace)
    };
    let start = match previous.and_then(|e| e.into_token()) {
        Some(t) if is_whitespace_text(&t) => t.text_range().start(),
        _ => target.text_range().start(),
    };
    TextRange::new(start, end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Document as AstDocument;
    use crate::syntax::parse;

    fn root(src: &str) -> Element {
        let p = parse(src);
        AstDocument::cast(p.syntax()).unwrap().root_element().unwrap()
    }

    /// Runs `f` against `src`'s root element and returns the whole-document
    /// text after applying the edits it returns — the shape most of this
    /// module's original tests already asserted on, kept via [`apply_edits`]
    /// now that the functions themselves return `Vec<Edit>`.
    fn applied(src: &str, f: impl FnOnce(&Element) -> Vec<Edit>) -> String {
        let el = root(src);
        let edits = f(&el);
        apply_edits(src, &edits)
    }

    #[test]
    fn every_special_character_round_trips_through_an_attribute() {
        for value in ["&Save", "a < b > c", "say \"hi\"", "it's", "two\nlines", "tab\there", "&amp; literally", "R&D; &#65;"] {
            for src in [r#"<Button Text="Ok"/>"#, "<Button Text='Ok'/>", "<Button/>"] {
                let out = applied(src, |el| set_attribute(el, "Text", value));
                let read = root(&out).attribute("Text").and_then(|a| a.value());
                assert_eq!(read.as_deref(), Some(value), "{src} -> {out}");
            }
        }
        let out = applied(r#"<Button Text="Ok"/>"#, |el| set_attribute(el, "Text", "&Save"));
        assert_eq!(out, r#"<Button Text="&amp;Save"/>"#);
    }

    #[test]
    fn set_existing_attribute_touches_only_its_value() {
        let out = applied(r#"<Button Text="Ok" Dock="Left"/>"#, |el| set_attribute(el, "Text", "Annuler"));
        assert_eq!(out, r#"<Button Text="Annuler" Dock="Left"/>"#);
    }

    #[test]
    fn set_existing_attribute_preserves_single_quotes() {
        let out = applied(r#"<Button Text='Ok'/>"#, |el| set_attribute(el, "Text", "Annuler"));
        assert_eq!(out, r#"<Button Text='Annuler'/>"#);
    }

    #[test]
    fn set_new_attribute_is_inserted_before_the_closer() {
        let out = applied(r#"<Button Text="Ok"/>"#, |el| set_attribute(el, "Dock", "Left"));
        assert_eq!(out, r#"<Button Text="Ok" Dock="Left"/>"#);
    }

    #[test]
    fn set_new_attribute_on_open_tag_form() {
        let out = applied(r#"<Panel Dock="Fill"></Panel>"#, |el| set_attribute(el, "Height", "56"));
        assert_eq!(out, r#"<Panel Dock="Fill" Height="56"></Panel>"#);
    }

    #[test]
    fn set_attribute_escapes_the_quote_character() {
        let out = applied(r#"<Label Text="a"/>"#, |el| set_attribute(el, "Text", r#"say "hi""#));
        assert_eq!(out, r#"<Label Text="say &quot;hi&quot;"/>"#);
    }

    #[test]
    fn set_attribute_returns_exactly_one_edit() {
        let el = root(r#"<Button Text="Ok"/>"#);
        assert_eq!(set_attribute(&el, "Text", "Annuler").len(), 1);
    }

    #[test]
    fn remove_attribute_drops_its_leading_space_too() {
        let out = applied(r#"<Button Text="Ok" Dock="Left"/>"#, |el| remove_attribute(el, "Dock"));
        assert_eq!(out, r#"<Button Text="Ok"/>"#);
    }

    #[test]
    fn remove_first_attribute_leaves_the_rest_untouched() {
        let out = applied(r#"<Button Text="Ok" Dock="Left"/>"#, |el| remove_attribute(el, "Text"));
        assert_eq!(out, r#"<Button Dock="Left"/>"#);
    }

    #[test]
    fn remove_missing_attribute_is_a_no_op() {
        let el = root(r#"<Button Text="Ok"/>"#);
        assert!(remove_attribute(&el, "Nope").is_empty());
    }

    #[test]
    fn insert_child_into_self_closing_parent() {
        let out = applied(r#"<Card Title="x"/>"#, |el| insert_child(el, 0, r#"<Label Text="hi"/>"#));
        assert_eq!(out, r#"<Card Title="x"><Label Text="hi"/></Card>"#);
    }

    #[test]
    fn insert_child_at_start() {
        let out = applied(r#"<Stack><A/><B/></Stack>"#, |el| insert_child(el, 0, "<Z/>"));
        assert_eq!(out, r#"<Stack><Z/><A/><B/></Stack>"#);
    }

    #[test]
    fn insert_child_in_middle() {
        let out = applied(r#"<Stack><A/><B/></Stack>"#, |el| insert_child(el, 1, "<Z/>"));
        assert_eq!(out, r#"<Stack><A/><Z/><B/></Stack>"#);
    }

    #[test]
    fn insert_child_past_end_appends() {
        let out = applied(r#"<Stack><A/><B/></Stack>"#, |el| insert_child(el, 99, "<Z/>"));
        assert_eq!(out, r#"<Stack><A/><B/><Z/></Stack>"#);
    }

    #[test]
    fn remove_child_drops_its_leading_whitespace() {
        let out = applied("<Stack>\n  <A/>\n  <B/>\n</Stack>", |el| remove_child(el, 0));
        assert_eq!(out, "<Stack>\n  <B/>\n</Stack>");
    }

    #[test]
    fn remove_last_child() {
        let out = applied("<Stack>\n  <A/>\n  <B/>\n</Stack>", |el| remove_child(el, 1));
        assert_eq!(out, "<Stack>\n  <A/>\n</Stack>");
    }

    #[test]
    fn remove_child_out_of_range_is_a_no_op() {
        let el = root("<Stack><A/></Stack>");
        assert!(remove_child(&el, 5).is_empty());
    }

    #[test]
    fn move_child_earlier() {
        let out = applied("<Stack><A/><B/><C/></Stack>", |el| move_child(el, 2, 0));
        assert_eq!(out, "<Stack><C/><A/><B/></Stack>");
    }

    #[test]
    fn move_child_later() {
        let out = applied("<Stack><A/><B/><C/></Stack>", |el| move_child(el, 0, 2));
        assert_eq!(out, "<Stack><B/><C/><A/></Stack>");
    }

    #[test]
    fn move_child_to_same_index_is_a_no_op() {
        let el = root("<Stack><A/><B/></Stack>");
        assert!(move_child(&el, 1, 1).is_empty());
    }

    #[test]
    fn move_child_with_whitespace_does_not_duplicate_it() {
        let out = applied("<Stack>\n  <A/>\n  <B/>\n  <C/>\n</Stack>", |el| move_child(el, 0, 2));
        // `A` keeps its own line and indentation, after `C`.
        assert_eq!(out, "<Stack>\n  <B/>\n  <C/>\n  <A/>\n</Stack>");
    }

    #[test]
    fn move_child_to_the_front_keeps_one_element_per_line() {
        let out = applied("<Stack>\n  <A/>\n  <B/>\n  <C/>\n</Stack>", |el| move_child(el, 2, 0));
        assert_eq!(out, "<Stack>\n  <C/>\n  <A/>\n  <B/>\n</Stack>");
    }

    #[test]
    fn move_child_into_the_middle_keeps_one_element_per_line() {
        let out = applied("<Stack>\n  <A/>\n  <B/>\n  <C/>\n</Stack>", |el| move_child(el, 0, 1));
        assert_eq!(out, "<Stack>\n  <B/>\n  <A/>\n  <C/>\n</Stack>");
    }

    #[test]
    fn move_element_within_the_same_parent_matches_move_child() {
        let src = "<Stack><A/><B/><C/></Stack>";
        let el = root(src);
        let out = apply_edits(src, &move_element(&el, 2, &el, 0));
        assert_eq!(out, "<Stack><C/><A/><B/></Stack>");
    }

    #[test]
    fn move_element_across_parents() {
        let src = r#"<Stack><Panel x:Name="left"><A/></Panel><Panel x:Name="right"><B/></Panel></Stack>"#;
        let p = parse(src);
        let doc = AstDocument::cast(p.syntax()).unwrap();
        let stack = doc.root_element().unwrap();
        let mut panels = stack.children();
        let left = panels.next().unwrap();
        let right = panels.next().unwrap();
        let a = left.children().next().unwrap();
        assert_eq!(a.name().as_deref(), Some("A"));

        let edits = move_element(&left, 0, &right, 0);
        let out = apply_edits(src, &edits);
        assert_eq!(
            out,
            r#"<Stack><Panel x:Name="left"></Panel><Panel x:Name="right"><A/><B/></Panel></Stack>"#
        );
    }

    #[test]
    fn move_element_into_its_own_subtree_is_a_no_op() {
        let src = "<Stack><Panel><A/></Panel></Stack>";
        let p = parse(src);
        let doc = AstDocument::cast(p.syntax()).unwrap();
        let stack = doc.root_element().unwrap();
        let panel = stack.children().next().unwrap();
        // Moving `Panel` itself to be a child of its own child `A` makes no
        // sense and must not corrupt the document.
        let a = panel.children().next().unwrap();
        assert!(move_element(&stack, 0, &a, 0).is_empty());
    }

    #[test]
    fn move_element_out_of_range_is_a_no_op() {
        let src = "<Stack><Panel/></Stack>";
        let p = parse(src);
        let doc = AstDocument::cast(p.syntax()).unwrap();
        let stack = doc.root_element().unwrap();
        assert!(move_element(&stack, 5, &stack, 0).is_empty());
    }

    #[test]
    fn rename_self_closing_element() {
        let out = applied(r#"<Button Text="Ok"/>"#, |el| rename_element(el, "Switch"));
        assert_eq!(out, r#"<Switch Text="Ok"/>"#);
    }

    #[test]
    fn rename_element_with_separate_end_tag_renames_both() {
        let out = applied(r#"<Panel Dock="Fill"></Panel>"#, |el| rename_element(el, "Card"));
        assert_eq!(out, r#"<Card Dock="Fill"></Card>"#);
    }

    #[test]
    fn rename_element_leaves_attributes_and_xname_value_untouched() {
        let out =
            applied(r#"<Button x:Name="ok_button" Text="Ok"/>"#, |el| rename_element(el, "Switch"));
        assert_eq!(out, r#"<Switch x:Name="ok_button" Text="Ok"/>"#);
    }

    fn names(list: &[&str]) -> std::collections::HashSet<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn unique_name_follows_the_winforms_numbering() {
        let taken = names(&["button1", "button2", "status"]);
        assert_eq!(unique_name("button1", &taken), "button3");
        assert_eq!(unique_name("status", &taken), "status2");
        assert_eq!(unique_name("free", &taken), "free");
    }

    #[test]
    fn collect_names_walks_the_whole_tree() {
        let el = root(r#"<Stack x:Name="a"><Card x:Name="b"><Button x:Name="c"/></Card><Button/></Stack>"#);
        assert_eq!(collect_names(&el), names(&["a", "b", "c"]));
    }

    #[test]
    fn insert_fragment_appends_on_its_own_indented_line() {
        let src = "<Stack>\n  <A/>\n</Stack>";
        let out = applied(src, |el| insert_fragment(el, 5, "<B/>", &names(&[])));
        assert_eq!(out, "<Stack>\n  <A/>\n  <B/>\n</Stack>");
    }

    #[test]
    fn insert_fragment_before_a_sibling_reindents_the_fragment() {
        let src = "<Card>\n    <Stack>\n        <A/>\n    </Stack>\n</Card>";
        let fragment = "<Panel>\n  <B/>\n</Panel>";
        let out = applied(src, |el| {
            let stack = el.children().next().unwrap();
            insert_fragment(&stack, 0, fragment, &names(&[]))
        });
        // The fragment keeps its own relative indentation, re-based on its new siblings' column.
        assert_eq!(out, "<Card>\n    <Stack>\n        <Panel>\n          <B/>\n        </Panel>\n        <A/>\n    </Stack>\n</Card>");
    }

    #[test]
    fn insert_fragment_into_an_empty_self_closing_container() {
        let out = applied("<Stack/>", |el| insert_fragment(el, 0, "<A/>", &names(&[])));
        assert_eq!(out, "<Stack>\n  <A/>\n</Stack>");
    }

    #[test]
    fn insert_fragment_into_an_empty_open_container_keeps_crlf() {
        let out = applied("<Card>\r\n  <Stack>\r\n  </Stack>\r\n</Card>", |el| {
            let stack = el.children().next().unwrap();
            insert_fragment(&stack, 0, "<A/>", &names(&[]))
        });
        assert_eq!(out, "<Card>\r\n  <Stack>\r\n    <A/>\r\n  </Stack>\r\n</Card>");
    }

    #[test]
    fn insert_fragment_renames_colliding_names_including_nested_ones() {
        let src = r#"<Stack><Button x:Name="ok1"/></Stack>"#;
        let fragment = r#"<Card x:Name="ok1"><Button x:Name="ok1"/></Card>"#;
        let out = applied(src, |el| insert_fragment(el, 1, fragment, &collect_names(el)));
        assert_eq!(out, r#"<Stack><Button x:Name="ok1"/><Card x:Name="ok2"><Button x:Name="ok3"/></Card></Stack>"#);
    }

    #[test]
    fn insert_fragment_rejects_anything_but_well_formed_elements() {
        let el = root("<Stack/>");
        assert!(insert_fragment(&el, 0, "not xml", &names(&[])).is_empty());
        assert!(insert_fragment(&el, 0, "<A>", &names(&[])).is_empty());
        assert!(insert_fragment(&el, 0, "", &names(&[])).is_empty());
    }

    #[test]
    fn insert_fragment_with_several_elements_puts_each_on_its_own_line() {
        let src = "<Stack>\n  <A/>\n</Stack>";
        let out = applied(src, |el| insert_fragment(el, 5, "<B/>\n<C>\n  <D/>\n</C>", &names(&[])));
        assert_eq!(out, "<Stack>\n  <A/>\n  <B/>\n  <C>\n    <D/>\n  </C>\n</Stack>");
    }

    #[test]
    fn insert_fragment_with_several_elements_renames_each_name_once() {
        let src = "<Stack>\n  <Button x:Name=\"ok\"/>\n</Stack>";
        let out = applied(src, |el| insert_fragment(el, 0, "<Button x:Name=\"ok\"/>\n<Button x:Name=\"ok\"/>", &names(&["ok"])));
        assert_eq!(out, "<Stack>\n  <Button x:Name=\"ok2\"/>\n  <Button x:Name=\"ok3\"/>\n  <Button x:Name=\"ok\"/>\n</Stack>");
    }

    #[test]
    fn reorder_children_moves_the_elements_and_keeps_the_layout() {
        let src = "<Stack>\n  <A/>\n  <B/>\n  <C/>\n</Stack>";
        // Bring A and B to the front (painted last): C, A, B.
        let out = applied(src, |el| reorder_children(el, &[2, 0, 1]));
        assert_eq!(out, "<Stack>\n  <C/>\n  <A/>\n  <B/>\n</Stack>");
    }

    #[test]
    fn reorder_children_rejects_anything_but_a_permutation() {
        let el = root("<Stack><A/><B/></Stack>");
        assert!(reorder_children(&el, &[0]).is_empty());
        assert!(reorder_children(&el, &[0, 0]).is_empty());
        assert!(reorder_children(&el, &[0, 2]).is_empty());
        assert!(reorder_children(&el, &[0, 1]).is_empty(), "the identity changes nothing");
    }

    #[test]
    fn wrap_element_indents_it_one_level_under_the_new_container() {
        let src = "<Card>\n  <Stack>\n    <A/>\n  </Stack>\n</Card>";
        let out = applied(src, |el| wrap_element(&el.children().next().unwrap(), "ScrollArea"));
        assert_eq!(out, "<Card>\n  <ScrollArea>\n    <Stack>\n      <A/>\n    </Stack>\n  </ScrollArea>\n</Card>");
    }

    #[test]
    fn wrap_inline_element_stays_inline() {
        let out = applied("<Stack><A/></Stack>", |el| wrap_element(&el.children().next().unwrap(), "Card"));
        assert_eq!(out, "<Stack><Card><A/></Card></Stack>");
    }

    #[test]
    fn wrap_element_rejects_an_invalid_container_name() {
        let el = root("<Stack><A/></Stack>");
        assert!(wrap_element(&el.children().next().unwrap(), "<Bad>").is_empty());
    }

    #[test]
    fn unwrap_element_lifts_the_children_one_level() {
        let src = "<Stack>\n  <Card>\n    <A/>\n    <!-- keep -->\n    <B/>\n  </Card>\n</Stack>";
        let out = applied(src, |el| unwrap_element(&el.children().next().unwrap()));
        assert_eq!(out, "<Stack>\n  <A/>\n  <!-- keep -->\n  <B/>\n</Stack>");
    }

    #[test]
    fn unwrap_root_needs_exactly_one_child() {
        let out = applied("<Card>\n  <Stack>\n    <A/>\n  </Stack>\n</Card>", unwrap_element);
        assert_eq!(out, "<Stack>\n  <A/>\n</Stack>");
        let el = root("<Stack><A/><B/></Stack>");
        assert!(unwrap_element(&el).is_empty());
    }

    #[test]
    fn unwrap_element_without_children_is_a_no_op() {
        let el = root("<Stack><A/></Stack>");
        assert!(unwrap_element(&el.children().next().unwrap()).is_empty());
    }

    #[test]
    fn apply_edits_composes_disjoint_ranges_regardless_of_order() {
        let src = r#"<Button Text="Ok" Dock="Left"/>"#;
        let el = root(src);
        let mut edits = set_attribute(&el, "Text", "Annuler");
        edits.extend(set_attribute(&el, "Dock", "Right"));
        let out = apply_edits(src, &edits);
        assert_eq!(out, r#"<Button Text="Annuler" Dock="Right"/>"#);
    }
}
