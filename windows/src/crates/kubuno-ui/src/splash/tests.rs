use super::*;

#[test]
fn the_version_line_reads_well() {
    assert_eq!(format_version("0.1.0-alpha", None), "Version 0.1.0-alpha");
    assert_eq!(format_version(" 0.1.0 ", Some("")), "Version 0.1.0");
    assert_eq!(format_version("0.1.0", Some("0.1.0")), "Version 0.1.0");
    // The git build identifier does not repeat the version it starts with.
    assert_eq!(format_version("0.1.0", Some("0.1.0-42.gabc1234")), "Version 0.1.0 · build 42.gabc1234");
    assert_eq!(format_version("0.1.0-alpha", Some("0.1.0-42.gabc1234.dirty.20261001")), "Version 0.1.0-alpha · build 42.gabc1234.dirty.20261001");
    assert_eq!(format_version("0.1.0", Some("nightly-7")), "Version 0.1.0 · build nightly-7");
    assert_eq!(format_version("", Some("abc")), "Build abc");
    assert_eq!(format_version("", None), "");
}

#[test]
fn the_product_name_splits_into_family_and_application() {
    assert_eq!(split_product("Kubuno Drive"), ("Kubuno".into(), "Drive".into()));
    assert_eq!(split_product("Kubuno  Documents "), ("Kubuno".into(), "Documents".into()));
    assert_eq!(split_product("Kubuno"), (String::new(), "Kubuno".into()));
    assert_eq!(split_product("Paint Sharp"), (String::new(), "Paint Sharp".into()));
}

#[test]
fn the_legal_line_names_the_licence() {
    assert_eq!(legal_line("AGPL-3.0-or-later"), "© Kubuno contributors · AGPL-3.0-or-later");
    assert_eq!(legal_line("MIT"), "© Kubuno contributors · MIT");
    assert_eq!(legal_line(""), "© Kubuno contributors");
}

#[test]
fn the_builder_follows_the_artwork_until_told_otherwise() {
    let s = SplashScreen::new().artwork(Artwork::Chat);
    assert_eq!(s.content().product, "Kubuno Chat");
    assert_eq!(s.content().status, "Démarrage de Kubuno Chat…");
    assert_eq!(s.content().tagline, Artwork::Chat.tagline());
    let s = SplashScreen::new().product("Kubuno Messagerie").artwork(Artwork::Chat).version("1.2.3").build("1.2.3-5.g0ff1ce").license("MIT");
    assert_eq!(s.content().product, "Kubuno Messagerie", "a product set by hand is kept");
    assert_eq!(s.content().status, "Démarrage de Kubuno Messagerie…");
    assert_eq!(s.content().version, "Version 1.2.3 · build 5.g0ff1ce");
    assert_eq!(s.content().legal, "© Kubuno contributors · MIT");
    let s = SplashScreen::new().status("Préparation…").artwork(Artwork::Drive);
    assert_eq!(s.content().status, "Préparation…", "a status set by hand is kept");
}

#[test]
fn a_disabled_splash_is_inert() {
    let splash = SplashScreen::new().enabled(false).show();
    assert!(!splash.is_shown());
    splash.set_status("x");
    splash.set_progress(0.5);
    splash.close();
    assert!(splash.time_to_first_paint().is_none());
    assert!(splash.wait_closed(Duration::from_millis(1)));
}

fn timeline() -> Timeline {
    Timeline::new(1500, 30_000, 200, 300)
}

#[test]
fn nothing_shows_before_the_first_frame() {
    let t = timeline();
    assert_eq!(t.opacity(10), 0.0);
    assert!(!t.finished(100_000));
}

#[test]
fn it_fades_in_then_holds() {
    let mut t = timeline();
    t.shown(1000);
    assert_eq!(t.opacity(1000), 0.0);
    let half = t.opacity(1100);
    assert!(half > 0.5 && half < 1.0, "eased out: {half}");
    assert_eq!(t.opacity(1200), 1.0);
    assert_eq!(t.opacity(5000), 1.0);
    assert!(t.animating(1100) && !t.animating(1300));
}

#[test]
fn a_ready_application_waits_for_the_minimum_time() {
    let mut t = timeline();
    t.shown(1000);
    // Ready 300 ms after the splash appeared: it stays until 1.5 s.
    t.request_close(1300);
    t.update(1300);
    assert!(!t.closing());
    t.update(2499);
    assert!(!t.closing());
    t.update(2500);
    assert!(t.closing());
    assert_eq!(t.opacity(2500), 1.0);
    assert!(t.opacity(2650) < 1.0);
    assert!(!t.finished(2799));
    assert!(t.finished(2800));
    assert_eq!(t.opacity(2800), 0.0);
}

#[test]
fn a_slow_application_closes_it_when_ready() {
    let mut t = timeline();
    t.shown(0);
    t.update(4000);
    assert!(!t.closing(), "nothing asked yet");
    t.request_close(4000);
    t.update(4016);
    assert!(t.closing());
    assert!(t.finished(4300));
    // A second request does not move it.
    t.request_close(9000);
    assert!(t.finished(4300));
}

#[test]
fn a_late_tick_starts_the_fade_where_it_should_have() {
    let mut t = timeline();
    t.shown(0);
    t.request_close(100);
    // The splash thread was busy until 1.7 s: the fade started at 1.5 s.
    t.update(1700);
    assert!(t.finished(1800));
}

#[test]
fn a_click_dismisses_it_at_once() {
    let mut t = timeline();
    t.shown(0);
    t.dismiss(400);
    assert!(t.closing());
    assert!(t.finished(700));
}

#[test]
fn it_never_stays_forever() {
    let mut t = timeline();
    t.shown(0);
    t.update(29_999);
    assert!(!t.closing());
    t.update(30_000);
    assert!(t.closing());
    assert!(t.finished(30_300));
}

#[test]
fn without_animations_it_appears_and_goes_at_once() {
    let mut t = Timeline::new(1500, 30_000, 0, 0);
    t.shown(0);
    assert_eq!(t.opacity(0), 1.0);
    t.request_close(0);
    t.update(1500);
    assert!(t.finished(1500));
}

#[test]
fn the_maximum_is_never_below_the_minimum() {
    let t = Timeline::new(5000, 1000, 0, 0);
    assert_eq!(t.max_ms, 5000);
}
