//! Tests of the designer's tolerant compilation (`super`, vskubuno `docs/DESIGNER.md` §17).

use super::*;
use crate::runtime::{DesignRenderState, Runtime};

fn tolerant(text: &str) -> TolerantCompile {
    compile_tolerant(text, None, &Vec::new, true)
}

#[test]
fn a_clean_view_compiles_as_written() {
    let r = tolerant(r#"<Stack><Button Text="Ok"/></Stack>"#);
    assert!(r.view.is_some());
    assert!(r.diagnostics.is_empty() && r.issues.is_empty(), "{:?} {:?}", r.diagnostics, r.issues);
    assert_eq!(r.built_text, r#"<Stack><Button Text="Ok"/></Stack>"#);
}

#[test]
fn an_unknown_element_becomes_a_placeholder_at_its_place() {
    let text = r#"<Panel><Button Text="Ok" X="8" Y="8"/><Frobnicator X="20" Y="40" Width="120" Height="30" Colour="red"><Label Text="inside"/></Frobnicator><Label Text="after"/></Panel>"#;
    let r = tolerant(text);
    assert!(r.view.is_some(), "the valid part still renders");
    assert_eq!(r.diagnostics.len(), 1, "{:?}", r.diagnostics);
    assert!(r.diagnostics[0].message.contains("unknown element `<Frobnicator>`"));
    assert_eq!(r.issues, vec![DesignIssue { element_id: "1".into(), message: "unknown element `<Frobnicator>`".into(), placeholder: true, attribute: None, range: Some(at(text, "Frobnicator")) }]);
    // Its place (X/Y/Width/Height) is kept, its other attributes dropped, its child still built inside it.
    assert!(r.built_text.contains(r#"<__DesignPlaceholderFlow __Element="Frobnicator""#), "{}", r.built_text);
    assert!(r.built_text.contains(r#"X="20" Y="40" Width="120" Height="30">"#), "{}", r.built_text);
    assert!(!r.built_text.contains("Colour"), "{}", r.built_text);
    assert!(r.built_text.contains(r#"<Label Text="inside"/></__DesignPlaceholderFlow><Label Text="after"/>"#), "{}", r.built_text);
    // Element ids are unchanged: the label after it is still `2`.
    let parse = syntax::parse(&r.built_text);
    let doc = Document::cast(parse.syntax()).unwrap();
    assert_eq!(doc.resolve_id("2").and_then(|e| e.attribute("Text")).and_then(|a| a.value()).as_deref(), Some("after"));
    assert_eq!(doc.resolve_id("1.0").and_then(|e| e.attribute("Text")).and_then(|a| a.value()).as_deref(), Some("inside"));
}

#[test]
fn an_unknown_root_element_still_renders_its_children() {
    let r = tolerant(r#"<NewShell><Button Text="Ok" Dock="Top"/></NewShell>"#);
    assert!(r.view.is_some());
    assert!(r.built_text.starts_with("<__DesignPlaceholder "), "positioned children keep a Dock/Anchor layout: {}", r.built_text);
}

#[test]
fn a_bad_attribute_value_falls_back_to_its_default_with_a_marker() {
    let r = tolerant(r#"<Stack><Button Text="Ok" Variant="Nope" Width="wide"/><Switch On="yes"/></Stack>"#);
    assert!(r.view.is_some());
    assert_eq!(r.diagnostics.len(), 3, "{:?}", r.diagnostics);
    assert_eq!(r.built_text, r#"<Stack><Button Text="Ok"/><Switch/></Stack>"#);
    let ids: Vec<&str> = r.issues.iter().map(|i| i.element_id.as_str()).collect();
    assert_eq!(ids, vec!["0", "0", "1"], "{:?}", r.issues);
    assert!(r.issues[0].message.ends_with("(ignored in the preview)"), "{}", r.issues[0].message);
}

#[test]
fn a_broken_binding_shows_the_property_default() {
    // The validator does not read inside `{…}`: the builder refuses it, and the property is dropped.
    let text = r#"<Stack><Switch On="{Notbinding}"/><Label Text="{Binding Name}"/></Stack>"#;
    let r = tolerant(text);
    assert!(r.view.is_some());
    assert_eq!(r.built_text, r#"<Stack><Switch/><Label Text="{Binding Name}"/></Stack>"#);
    assert_eq!(r.diagnostics.len(), 1, "{:?}", r.diagnostics);
    assert!(r.diagnostics[0].message.contains("malformed binding"), "{}", r.diagnostics[0].message);
    // Positioned in the text as written: the value of `On`.
    let start = usize::from(r.diagnostics[0].range.start());
    assert!(text[start..].starts_with("{Notbinding}"), "{:?}", r.diagnostics[0]);
}

#[test]
fn children_a_parent_cannot_hold_are_not_shown_and_a_wrong_closing_tag_is_mended() {
    let r = tolerant(r#"<Stack><Card><Label Text="a"/><Label Text="b"/><Label Text="c"/></Card><Switch><Label Text="x"/></Switch></Stack>"#);
    assert!(r.view.is_some());
    assert_eq!(r.built_text, r#"<Stack><Card><Label Text="a"/></Card><Switch></Switch></Stack>"#);
    let r = tolerant(r#"<Stack><Label Text="a"></Button></Stack>"#);
    assert!(r.view.is_some());
    assert_eq!(r.built_text, r#"<Stack><Label Text="a"></Label></Stack>"#);
}

#[test]
fn a_gated_child_in_the_wrong_parent_becomes_a_placeholder() {
    let r = tolerant(r#"<Stack><TabItem Header="Oops"><Button Text="x"/></TabItem></Stack>"#);
    assert!(r.view.is_some());
    assert!(r.built_text.contains(r#"__Element="TabItem""#), "{}", r.built_text);
    assert!(r.built_text.contains(r#"<Button Text="x"/>"#), "{}", r.built_text);
}

#[test]
fn a_malformed_text_with_no_previous_preview_renders_what_was_recovered() {
    let r = tolerant(r#"<Stack><Button Text="Ok"/><Label Text="half"#);
    assert!(r.malformed && r.recovered);
    assert!(r.view.is_some(), "{}", r.built_text);
    assert!(r.built_text.starts_with(r#"<Stack><Button Text="Ok"/>"#), "{}", r.built_text);
    assert!(!r.diagnostics.is_empty());
}

#[test]
fn a_malformed_text_without_recovery_builds_nothing() {
    let r = compile_tolerant(r#"<Stack><Button Text="Ok"/><Label Text="half"#, None, &Vec::new, false);
    assert!(r.malformed && !r.recovered && r.view.is_none());
    assert!(!r.diagnostics.is_empty());
}

#[test]
fn nothing_to_recover_is_empty() {
    let r = tolerant("   ");
    assert!(r.view.is_none());
}

#[test]
fn the_runtime_keeps_the_last_good_preview_over_a_malformed_text() {
    let mut rt = Runtime::new();
    let first = rt.reload_for_design(r#"<Stack><Button Text="Ok"/></Stack>"#);
    assert_eq!(first.state, DesignRenderState::Clean);
    let tolerant = rt.reload_for_design(r#"<Stack><Button Text="Ok"/><Frob/></Stack>"#);
    assert_eq!(tolerant.state, DesignRenderState::Tolerant);
    assert_eq!(tolerant.issues.len(), 1);
    // Typing: not well-formed. The previous (tolerant) preview stays.
    let stale = rt.reload_for_design(r#"<Stack><Button Text="Ok"/><Fro"#);
    assert_eq!(stale.state, DesignRenderState::Stale);
    assert!(rt.has_view());
    assert!(!stale.diagnostics.is_empty());
    assert!(stale.issues.is_empty());
    let fixed = rt.reload_for_design(r#"<Stack><Button Text="Ok"/></Stack>"#);
    assert_eq!(fixed.state, DesignRenderState::Clean);
}

#[test]
fn a_broken_file_opened_first_is_recovered_then_kept() {
    let mut rt = Runtime::new();
    let opened = rt.reload_for_design(r#"<Stack><Button Text="Ok"/><Label "#);
    assert_eq!(opened.state, DesignRenderState::Recovered);
    assert!(rt.has_view());
    let empty = Runtime::new().reload_for_design("");
    assert_eq!(empty.state, DesignRenderState::Empty);
}

#[test]
fn the_placeholder_names_are_found_but_listed_nowhere() {
    assert!(registry::lookup(PLACEHOLDER_ELEMENT).is_some());
    assert!(registry::lookup(PLACEHOLDER_FLOW_ELEMENT).is_some());
    assert!(registry::all().iter().all(|m| !is_placeholder(m.name)));
    // A view that names it itself still validates (it is never offered, but not an error either).
    assert!(validate::validate_with_default_registry(&syntax::parse(r#"<__DesignPlaceholder __Element="X"/>"#)).is_empty());
}

/// The range of the first `needle` in `text`.
fn at(text: &str, needle: &str) -> TextRange {
    let start = text.find(needle).unwrap();
    TextRange::new(TextSize::from(start as u32), TextSize::from((start + needle.len()) as u32))
}

#[test]
fn an_ignored_attribute_s_marker_knows_where_it_is_written() {
    let text = r#"<Stack><Switch On="yes"/></Stack>"#;
    let r = tolerant(text);
    assert_eq!(r.issues.len(), 1);
    assert_eq!(r.issues[0].element_id, "0");
    assert_eq!(r.issues[0].attribute.as_deref(), Some("On"));
    assert_eq!(r.issues[0].range, Some(at(text, "yes")));
}
