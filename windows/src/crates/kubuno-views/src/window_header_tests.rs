//! The title bar's standard items (`ShowSearch`, `ShowNotifications`, `ShowSettings`, `ShowHelp`, `ShowWaffle`,
//! `ShowAccount` on the view's root, vskubuno `docs/SHELL-CONTROLS.md` §5): their defaults per window kind, the
//! elements they become, the band's neutral look, and the title-bar regions they join (centre region, gaps, RTL).

use std::cell::RefCell;
use std::rc::Rc;

use super::*;
use crate::ast::Document;
use crate::controls::Button;
use crate::registry::families::containers::{band_item_rects, band_slot_widths, BandItem};
use crate::registry::project::{register_class, ClassKind, ClassRegistration};
use kubuno_controls::window_chrome::{self as wc, ChromeStyle, SlotWidths, SystemButtons};

fn root(src: &str) -> Element {
    let parse = crate::syntax::parse(src);
    Document::cast(parse.syntax()).and_then(|d| d.root_element()).expect("a root element")
}

/// A stand-in for the cluster's class (the real one lives in a crate this one never links).
#[derive(crate::component::Component, Default)]
#[kubuno(extends = Button, overrides(Control))]
struct TestHeaderCluster {
    base: Button,
}

impl crate::component::Control for TestHeaderCluster {}

fn create_cluster() -> Option<Rc<RefCell<dyn crate::component::Component>>> {
    Some(Rc::new(RefCell::new(TestHeaderCluster::default())))
}

static CLUSTER: ClassRegistration = ClassRegistration {
    name: "TestHeaderCluster",
    crate_name: "test_shell",
    kind: ClassKind::Control,
    doc: "The header's cluster, for the tests.",
    extends: "Button",
    chain: <TestHeaderCluster as crate::component::Lineage>::CHAIN,
    create: create_cluster,
    properties: &[],
    events: &[],
    default_event: None,
    default_property: None,
    toolbox_category: None,
    toolbox_icon: None,
    browsable: true,
    view: None,
    view_path: None,
    view_dir: None,
    source_file: "test_header_cluster.rs",
};

#[test]
fn the_header_items_are_off_by_default_for_every_window_kind_and_the_view_wins() {
    for kind in crate::registry::common::WINDOW_KINDS {
        let plain = root(&format!(r#"<Panel WindowKind="{kind}"/>"#));
        assert!(HeaderSpec::read(&plain).is_empty(), "{kind}: nothing unless asked");
        assert!(!FormSpec::read(&plain, None).header_items, "{kind}");
        let asked = root(&format!(r#"<Panel WindowKind="{kind}" ShowWaffle="true" ShowAccount="true"/>"#));
        let spec = HeaderSpec::read(&asked);
        assert!(spec.has_cluster() && spec.search.is_none(), "{kind}: what the view writes wins");
        assert!(FormSpec::read(&asked, None).header_items, "{kind}");
    }
    // `false` written is the same as nothing.
    assert!(HeaderSpec::read(&root(r#"<Panel ShowSearch="false" ShowHelp="false"/>"#)).is_empty());
    // The registry agrees: every property defaults to off.
    for name in ["ShowSearch", "ShowNotifications", "ShowSettings", "ShowHelp", "ShowWaffle", "ShowAccount"] {
        let p = crate::registry::view_property(name).expect(name);
        assert_eq!((p.default, p.category), ("false", Some("Title Bar")), "{name}");
    }
    for event in ["OnSearchClicked", "OnNotificationsClicked", "OnSettingsClicked", "OnHelpClicked"] {
        assert!(crate::registry::view_event(event).is_some(), "{event}");
    }
}

#[test]
fn the_items_become_a_search_button_and_the_cluster_with_the_view_s_values_and_handlers() {
    let r = root(
        r#"<Panel ShowSearch="true" OnSearchClicked="find" ShowNotifications="true" ShowHelp="false" ShowWaffle="{Binding Signed}" ShowAccount="true" UnreadCount="{Binding Unread}" OnNotificationsClicked="bell_click" OnSettingsClicked="settings_click"/>"#,
    );
    let spec = HeaderSpec::read(&r);
    assert_eq!(spec.cluster_width(), 3.0 * HEADER_BUTTON_COMPACT + HEADER_AVATAR_GAP + HEADER_TRAILING_GAP, "bell, waffle (bound: room kept), avatar");
    let search = spec.search_xml().expect("a search button");
    assert!(search.starts_with(r#"<IconButton x:Name="__kb_header_search" Icon="Search""#) && search.contains(r#"OnClick="find""#) && search.contains(r#"Width="30""#), "{search}");
    let cluster = spec.cluster_xml(true, false).expect("the cluster");
    for part in [
        r#"<HeaderActions Compact="true" ShowNotifications="true""#,
        r#"ShowSettings="false""#,
        r#"ShowHelp="false""#,
        r#"ShowWaffle="{Binding Signed}""#,
        r#"ShowAccount="true""#,
        r#"UnreadCount="{Binding Unread}""#,
        r#"OnNotificationsClicked="bell_click""#,
        r#"OnSettingsClicked="settings_click""#,
        r#"Compact="true""#,
        r#"Width="100" Height="50"/>"#,
    ] {
        assert!(cluster.contains(part), "{part} in {cluster}");
    }
    assert!(!cluster.contains("OnHelpClicked"), "no handler written, none passed on");
    // A bound search button hides itself with its binding.
    let bound = HeaderSpec::read(&root(r#"<Panel ShowSearch="{Binding CanSearch}"/>"#));
    assert!(bound.search_xml().is_some_and(|x| x.contains(r#"Visible="{Binding CanSearch}""#)));
    assert!(bound.cluster_xml(true, false).is_none(), "no cluster asked");
}

#[test]
fn a_missing_cluster_class_is_reported_and_shown_as_a_placeholder_in_the_designer_only() {
    let spec = HeaderSpec::read(&root(r#"<Panel ShowWaffle="true"/>"#));
    assert!(spec.cluster_xml(false, false).is_none(), "left out at run time");
    let placeholder = spec.cluster_xml(false, true).expect("a placeholder in the designer");
    assert!(placeholder.starts_with(&format!("<{}", crate::tolerant::PLACEHOLDER_ELEMENT)), "{placeholder}");
    assert!(placeholder.contains(r#"__Element="HeaderActions""#) && placeholder.contains("shell controls"), "{placeholder}");
    assert!(header_class_missing_message().contains(HEADER_ACTIONS_CLASS));
}

#[test]
fn the_view_root_builds_the_items_after_its_own_children_when_the_class_is_registered() {
    register_class(&CLUSTER);
    set_header_class_for_tests("TestHeaderCluster");
    let r = root(r#"<Panel ShowSearch="true" ShowSettings="true" OnSettingsClicked="go"><Label TitleBar.Region="Right" Text="x"/></Panel>"#);
    let mut cx = crate::props::BuildCx::new();
    let items = build_header_items(&r, &mut cx);
    assert_eq!(items.iter().map(|(_, size)| *size).collect::<Vec<_>>(), [(30.0, 30.0), (38.0, 50.0)], "the search button, then the cluster (one button)");
    // The whole view compiles with them (the root panel adds them to its children).
    assert!(crate::compile::compile(r#"<Panel ShowWaffle="true" ShowSearch="true"><Button Text="ok"/></Panel>"#).is_ok());
    set_header_class_for_tests(HEADER_ACTIONS_CLASS);
    // Not registered at run time: only the search button.
    let mut cx = crate::props::BuildCx::new();
    assert_eq!(build_header_items(&r, &mut cx).len(), 1);
    assert!(build_header_items(&root("<Panel/>"), &mut cx).is_empty());
}

#[test]
fn a_header_band_takes_the_web_header_s_neutral_look_unless_the_view_colours_it() {
    let vm = crate::binding::MapViewModel::default();
    let theme = kubuno_ui::Theme::light();
    let header = FormSpec::read(&root(r#"<Panel ShowAccount="true"/>"#), None).options_in(&vm, Some(&theme));
    let background = crate::style::parse_color("Background").ok().flatten().map(|c| c.resolve_with(&theme, false));
    assert_eq!(header.chrome.background.map(|c| (c.r, c.g, c.b)), background.map(|c| (c.r, c.g, c.b)));
    assert!(header.chrome.foreground.is_some(), "the text colour, not white");
    let plain = FormSpec::read(&root("<Panel/>"), None).options_in(&vm, Some(&theme));
    assert_eq!(plain.chrome.background, None, "the accent band as before");
    let coloured = FormSpec::read(&root(r#"<Panel ShowAccount="true" AccentColor="Danger"/>"#), None).options_in(&vm, Some(&theme));
    assert_ne!(coloured.chrome.background.map(|c| (c.r, c.g, c.b)), background.map(|c| (c.r, c.g, c.b)), "the view's colour wins");
    assert_eq!(coloured.chrome.foreground, None);
}

fn band(style: &ChromeStyle, slots: SlotWidths) -> wc::ChromeLayout {
    wc::layout(style, Rect::new(0.0, 0.0, 800.0, 600.0), true, SystemButtons::default(), slots)
}

fn item(region: TitleRegion, width: f32, joined: bool) -> BandItem {
    BandItem { region, width, height: 36.0, joined }
}

#[test]
fn the_regions_take_their_items_in_order_and_the_standard_items_side_by_side() {
    let items = [
        item(TitleRegion::Left, 36.0, false),
        item(TitleRegion::Center, 160.0, false),
        item(TitleRegion::Right, 36.0, false),
        // The search button and the cluster: joined to each other, not to the view's own button.
        item(TitleRegion::Right, 36.0, false),
        item(TitleRegion::Right, 110.0, true),
        item(TitleRegion::None, 50.0, false),
    ];
    let widths = band_slot_widths(&items);
    assert_eq!((widths.left, widths.center, widths.right), (36.0, 160.0, 36.0 + wc::BUTTON_GAP + 36.0 + 110.0));
    let l = band(&ChromeStyle { height: Some(64.0), ..ChromeStyle::default() }, widths);
    let rects = band_item_rects(&items, &l, false);
    let r = |i: usize| rects[i].expect("placed");
    assert_eq!(r(0).left, l.left.left);
    // The centre region is centred on the window, whatever the sides hold.
    assert_eq!((r(1).left + r(1).right) / 2.0, 400.0);
    assert_eq!(r(3).left, r(2).right + wc::BUTTON_GAP, "gap-1 between two controls");
    assert_eq!(r(4).left, r(3).right, "no gap inside the standard items");
    assert_eq!(r(4).right, l.right.right, "the cluster ends the right region");
    assert!(rects[5].is_none());
    // 36 tall, centred on the 64 DIP header; a 50 DIP band clips nothing either.
    assert_eq!((r(2).top, r(2).bottom), (14.0, 50.0));
    // The caption buttons stay clear of the region (the gap between them still drags the window).
    let first_caption = l.buttons.iter().map(|(_, b, _)| b.left).fold(f32::MAX, f32::min);
    assert!(l.right.right < first_caption, "{:?} vs {first_caption}", l.right);
    assert!(l.hit(r(4).right - 1.0, 32.0).is_none(), "a click on the avatar is not a caption button");
}

#[test]
fn a_right_to_left_window_mirrors_the_regions_and_their_order() {
    let items = [item(TitleRegion::Right, 36.0, false), item(TitleRegion::Right, 74.0, true), item(TitleRegion::Left, 36.0, false)];
    let widths = band_slot_widths(&items);
    let l = band(&ChromeStyle { right_to_left: true, ..ChromeStyle::default() }, widths);
    let rects = band_item_rects(&items, &l, true);
    let r = |i: usize| rects[i].expect("placed");
    // The caption buttons are on the left: the right region is next to them, read from the right.
    assert!(l.right.right < 400.0 && l.left.left > 400.0, "{:?} {:?}", l.left, l.right);
    assert_eq!(r(0).right, l.right.right, "the view's first item at the region's start (its right edge)");
    assert_eq!(r(1).right, r(0).left, "the cluster follows it, towards the caption buttons");
    assert_eq!(r(1).left, l.right.left);
    assert_eq!(r(2).right, l.left.right);
}

#[test]
fn the_designer_lays_the_items_out_in_its_own_band() {
    set_design_chrome(Some(DesignChrome {
        style: ChromeStyle { height: Some(64.0), ..ChromeStyle::default() },
        bounds: Rect::new(0.0, 0.0, 900.0, 500.0),
        has_icon: false,
        buttons: SystemButtons::default(),
    }));
    let l = title_bar_slots(SlotWidths { left: 0.0, center: 0.0, right: 182.0 }).expect("a band");
    assert_eq!(l.right.right - l.right.left, 182.0);
    assert_eq!(declared_slots().right, 182.0);
    set_design_chrome(None);
}

#[test]
fn a_toolbox_drop_on_the_title_bar_goes_to_the_region_under_the_pointer() {
    use crate::design::{band_drop_zones, EditOp, LayoutMap, ToolboxController};
    let l = band(&ChromeStyle { height: Some(64.0), ..ChromeStyle::default() }, SlotWidths::default());
    let zones = band_drop_zones(&l);
    assert_eq!(zones.map(|(r, _)| r), ["Left", "Center", "Right"]);
    assert!(zones[0].1.right <= zones[1].1.left && zones[1].1.right <= zones[2].1.left);
    assert!(zones[2].1.right <= l.buttons.iter().map(|(_, b, _)| b.left).fold(f32::MAX, f32::min), "never over the caption buttons");
    // Mirrored: the zone next to the caption buttons (now on the left) is the right region's.
    let rtl = band(&ChromeStyle { right_to_left: true, ..ChromeStyle::default() }, SlotWidths::default());
    let mirrored = band_drop_zones(&rtl);
    assert!(mirrored[2].1.left < mirrored[0].1.left, "{mirrored:?}");

    let parse = crate::syntax::parse(r#"<Panel Title="Main"><Label Text="x"/></Panel>"#);
    let doc = Document::cast(parse.syntax()).expect("a document");
    let mut toolbox = ToolboxController::new();
    toolbox.set_title_band(Some(l.clone()));
    toolbox.drag_enter("IconButton".into());
    let (_, hot) = toolbox.band_zones().expect("the zones show while dragging");
    assert!(hot.is_none());
    let right = zones[2].1;
    let target = toolbox.drag_over(&LayoutMap::new(), &doc, right.left + 5.0, 30.0).expect("a target").clone();
    assert!(target.valid && target.parent_id.is_empty() && target.index == 1);
    assert_eq!(toolbox.band_zones().and_then(|(_, hot)| hot), Some("Right"));
    match toolbox.drop() {
        Some(EditOp::InsertChild { xml, .. }) => assert_eq!(xml, r#"<IconButton TitleBar.Region="Right" Diameter="36" Glyph="18" Width="36" Height="36"/>"#),
        other => panic!("{other:?}"),
    }
    // The centre zone.
    toolbox.drag_enter("TextField".into());
    toolbox.drag_over(&LayoutMap::new(), &doc, 400.0, 30.0);
    assert!(matches!(toolbox.drop(), Some(EditOp::InsertChild { xml, .. }) if xml.contains(r#"TitleBar.Region="Center""#)));
}

#[test]
fn a_wide_centre_region_never_covers_the_header_s_buttons() {
    let style = ChromeStyle { height: Some(64.0), ..ChromeStyle::default() };
    let l = wc::layout(&style, Rect::new(0.0, 0.0, 1100.0, 440.0), true, SystemButtons::default(), SlotWidths { left: 36.0, center: 480.0, right: 254.0 });
    assert!(l.center.right <= l.right.left, "{:?} {:?}", l.center, l.right);
    assert!(l.center.left >= l.left.right, "{:?} {:?}", l.center, l.left);
    assert_eq!(l.center.right - l.center.left, 480.0, "pushed aside, not narrowed, while it fits");
    // No room: narrowed to what is left, its control cut to it.
    let narrow = wc::layout(&style, Rect::new(0.0, 0.0, 700.0, 440.0), false, SystemButtons::default(), SlotWidths { left: 0.0, center: 480.0, right: 254.0 });
    assert!(narrow.center.right <= narrow.right.left && narrow.center.right - narrow.center.left < 480.0);
    let rects = band_item_rects(&[item(TitleRegion::Center, 480.0, false)], &narrow, false);
    assert_eq!(rects[0].map(|r| r.right), Some(narrow.center.right));
}

#[test]
fn the_items_take_the_caption_buttons_size_in_a_usual_title_bar_and_36_in_the_tall_header() {
    let all = r#"ShowNotifications="true" ShowSettings="true" ShowHelp="true" ShowWaffle="true" ShowAccount="true" ShowSearch="true""#;
    let usual = HeaderSpec::read(&root(&format!("<Panel {all}/>")));
    assert!(!usual.tall());
    assert_eq!((usual.button(), usual.cluster_width(), usual.band_height), (30.0, 160.0, 50.0));
    assert!(usual.cluster_xml(true, false).is_some_and(|x| x.contains(r#"Compact="true""#) && x.contains(r#"Width="160" Height="50""#)));
    assert!(usual.search_xml().is_some_and(|x| x.contains(r#"Diameter="30" Glyph="16" Width="30" Height="30""#)));
    let rtl = HeaderSpec::read(&root(&format!(r#"<Panel RightToLeftLayout="true" {all}/>"#)));
    assert!(rtl.cluster_xml(true, false).is_some_and(|x| x.contains(r#"RightToLeft="true""#)), "the cluster mirrored too");
    assert!(!usual.cluster_xml(true, false).is_some_and(|x| x.contains("RightToLeft")));
    let tall = HeaderSpec::read(&root(&format!(r#"<Panel TitleBarHeight="64" {all}/>"#)));
    assert!(tall.tall());
    assert_eq!((tall.button(), tall.cluster_width()), (36.0, 190.0));
    assert!(tall.cluster_xml(true, false).is_some_and(|x| x.contains(r#"Compact="false""#) && x.contains(r#"Width="190" Height="64""#)));
    // A tool window's slim band: the buttons fit it.
    let tool = HeaderSpec::read(&root(&format!(r#"<Panel WindowKind="ToolWindow" {all}/>"#)));
    assert_eq!((tool.band_height, tool.button()), (32.0, 30.0));
}

#[test]
fn the_language_server_warns_when_the_cluster_class_is_not_linked() {
    let parse = crate::syntax::parse(r#"<Panel Title="x" ShowWaffle="true"/>"#);
    let w = crate::validate::warnings(&parse);
    assert!(w.iter().any(|d| d.message.contains(HEADER_ACTIONS_CLASS)), "{w:?}");
    let parse = crate::syntax::parse(r#"<Panel ShowSearch="true"/>"#);
    assert!(crate::validate::warnings(&parse).iter().all(|d| !d.message.contains(HEADER_ACTIONS_CLASS)), "the search button needs nothing");
}

#[test]
fn on_a_coloured_band_the_items_take_its_ink_and_the_pale_avatar() {
    for view in [
        r#"<Panel ShowWaffle="true" ShowAccount="true" AccentColor="Danger"/>"#,
        r#"<Panel ShowWaffle="true" ShowAccount="true"><Ribbon Dock="Top"/></Panel>"#,
    ] {
        let r = root(view);
        assert!(coloured_band(&r), "{view}");
        assert!(!FormSpec::read(&r, None).header_items, "no neutral look: {view}");
        let xml = HeaderSpec::read(&r).cluster_xml(true, false).expect("the cluster");
        assert!(xml.contains(r#"ForeColor="OnPrimary" AvatarTint="Accent""#), "{xml}");
    }
    let own = HeaderSpec::read(&root(r##"<Panel ShowSearch="true" TitleBarBackground="#102030" TitleBarForeground="#FFEECC"/>"##));
    assert!(own.search_xml().is_some_and(|x| x.contains(r##"ForeColor="#FFEECC""##)));
    // A ribbon that does not colour the band: the neutral header.
    let r = root(r#"<Panel ShowWaffle="true" TitleBarFollowsRibbon="false"><Ribbon Dock="Top"/></Panel>"#);
    assert!(!coloured_band(&r) && FormSpec::read(&r, None).header_items);
    assert!(HeaderSpec::read(&r).cluster_xml(true, false).is_some_and(|x| !x.contains("ForeColor")));
}
