//! Tests over the views themselves (`document_window.kbview`, `backstage_info.kbcontrol`) and their
//! resources — what `ribbon.rs`'s tests checked when the ribbon was declared in Rust: every icon is
//! embedded, no command is an unnamed icon, the tabs follow the web's order, the clipboard group
//! comes first; plus the view compiles and every `{Res}` it names exists in both languages.

use kubuno::views::ast::{AstNode, Document, Element};
use kubuno::views::syntax::parse;

const WINDOW: &str = include_str!("document_window.kbview");
const BACKSTAGE: &str = include_str!("../pages/backstage_info.kbcontrol");
const RES_EN: &str = include_str!("../resources/resources.kbres");
const RES_FR: &str = include_str!("../resources/resources.fr.kbres");

fn elements(text: &str) -> Vec<Element> {
    let parsed = parse(text);
    let root = Document::cast(parsed.syntax()).and_then(|d| d.root_element()).expect("a root element");
    root.syntax().descendants().filter_map(Element::cast).collect()
}

fn attr(e: &Element, name: &str) -> Option<String> {
    e.attribute(name).and_then(|a| a.value())
}

/// The `<Command>`s of the window, by `x:Name`.
fn commands() -> Vec<Element> {
    elements(WINDOW).into_iter().filter(|e| e.name().as_deref() == Some("Command")).collect()
}

/// Every icon name embedded in the shared asset files.
fn embedded_icon_names() -> Vec<String> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../drive/crates/drive-app-controls/assets/");
    let mut names = Vec::new();
    for file in ["lucide-icons.txt", "themed-icons.txt", "module-logos.txt"] {
        let path = format!("{root}{file}");
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("the icon asset {path} must be readable: {e}"));
        names.extend(text.lines().filter_map(|l| l.trim().strip_prefix("=== ")).filter_map(|r| r.split_whitespace().next()).map(str::to_string));
    }
    assert!(names.len() > 100, "the asset files parsed to only {} names", names.len());
    names
}

/// The resource keys a `.kbres` file defines.
fn resource_keys(text: &str) -> Vec<String> {
    elements(text).iter().filter_map(|e| attr(e, "Name")).collect()
}

#[test]
fn the_window_view_compiles() {
    // The view's own controls register when the library is linked (this test binary).
    let _ = (crate::PageCanvas::default(), crate::HorizontalRuler::default());
    let mut rt = kubuno::views::runtime::Runtime::new();
    let ok = rt.reload_from_text(WINDOW);
    assert!(ok, "{:#?}", rt.diagnostics());
}

#[test]
fn every_declared_icon_exists_in_the_asset_files() {
    // A name that is not embedded draws NOTHING, with no error anywhere.
    let names = embedded_icon_names();
    for e in elements(WINDOW) {
        for a in ["SmallIcon", "LargeIcon", "Icon"] {
            if let Some(icon) = attr(&e, a).filter(|i| !i.starts_with('{')) {
                assert!(names.contains(&icon), "`{}` declares the icon `{icon}`, which is not embedded", e.name().unwrap_or_default());
            }
        }
    }
}

#[test]
fn no_command_would_draw_a_blank_box_or_be_an_unnamed_icon() {
    let cmds = commands();
    for e in elements(WINDOW) {
        let name = e.name().unwrap_or_default();
        if !["RibbonButton", "RibbonToggleButton", "RibbonSplitButton", "RibbonMenuButton", "RibbonMenuItem"].contains(&name.as_str()) {
            continue;
        }
        // Its label (own, else its command's): the text shown, or the tooltip of an icon-only one.
        let command = attr(&e, "Command").and_then(|c| cmds.iter().find(|k| attr(k, "x:Name").as_deref() == Some(c.as_str())).cloned());
        let label = attr(&e, "Label").or_else(|| command.as_ref().and_then(|c| attr(c, "Label")));
        assert!(label.as_deref().is_some_and(|l| !l.is_empty()), "a `{name}` has no label ({:?})", attr(&e, "Command"));
        let icon = attr(&e, "SmallIcon").or_else(|| command.as_ref().and_then(|c| attr(c, "SmallIcon")));
        if name != "RibbonMenuItem" {
            assert!(icon.is_some(), "a `{name}` ({label:?}) has no icon");
        }
    }
}

#[test]
fn tabs_follow_the_web_order() {
    let tabs: Vec<String> = elements(WINDOW)
        .iter()
        .filter(|e| matches!(e.name().as_deref(), Some("RibbonTab") | Some("RibbonBackstage")))
        .filter_map(|e| attr(e, "x:Name"))
        .collect();
    assert_eq!(tabs, ["backstage", "tab_home", "tab_insert", "tab_layout", "tab_references", "tab_view", "tab_review"]);
}

#[test]
fn clipboard_is_the_first_group_of_home() {
    let all = elements(WINDOW);
    let home = all.iter().find(|e| attr(e, "x:Name").as_deref() == Some("tab_home")).expect("an Accueil tab");
    let first = home.syntax().children().filter_map(Element::cast).next().expect("a group");
    assert_eq!(attr(&first, "x:Name").as_deref(), Some("grp_clip"));
}

#[test]
fn the_ruler_command_switches_the_rulers() {
    let ruler = commands().into_iter().find(|c| attr(c, "x:Name").as_deref() == Some("cmd_ruler")).expect("cmd_ruler");
    assert_eq!(attr(&ruler, "IsCheckable").as_deref(), Some("true"));
    assert_eq!(attr(&ruler, "Checked").as_deref(), Some("{Binding ShowRuler, Mode=TwoWay}"));
    for name in ["ruler_column", "horizontal_ruler"] {
        let e = elements(WINDOW).into_iter().find(|e| attr(e, "x:Name").as_deref() == Some(name)).expect(name);
        assert_eq!(attr(&e, "Visible").as_deref(), Some("{Binding ShowRuler}"), "{name}");
    }
}

#[test]
fn every_resource_the_views_name_exists_in_both_languages() {
    let (en, fr) = (resource_keys(RES_EN), resource_keys(RES_FR));
    for text in [WINDOW, BACKSTAGE] {
        for part in text.split("{Res ").skip(1) {
            let key = part.split('}').next().unwrap_or_default().trim();
            assert!(en.iter().any(|k| k == key), "`{key}` is missing from resources.kbres");
            assert!(key == "app_icon" || fr.iter().any(|k| k == key), "`{key}` is missing from resources.fr.kbres");
        }
    }
    let mut seen = std::collections::HashSet::new();
    for k in &en {
        assert!(seen.insert(k), "`{k}` is defined twice");
    }
}
