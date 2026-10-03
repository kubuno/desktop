//! Runtime tests: culture fallback, lookups across sets, change notification. They share the
//! process-wide culture, so they run one at a time.

use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

static SERIAL: Mutex<()> = Mutex::new(());

static NEUTRAL: &str = r##"<Resources>
  <String Name="hello">Hello</String>
  <String Name="bye">Goodbye</String>
  <String Name="only_neutral">Neutral</String>
  <Image Name="logo" File="logo.png"/>
  <Image Name="dot" Format="png">AAEC</Image>
  <Color Name="accent" Value="#3366FF"/>
</Resources>"##;
static FR: &str = r#"<Resources><String Name="hello">Bonjour</String><String Name="bye">Au revoir</String><Image Name="logo" File="logo.fr.png"/></Resources>"#;
static FR_CA: &str = r#"<Resources><String Name="hello">Allô</String></Resources>"#;
static DE_DE: &str = r#"<Resources><String Name="hello">Hallo</String><Image Name="hello2" File="x.png"/></Resources>"#;

static EMBEDDED: EmbeddedSet = EmbeddedSet {
    name: "testres",
    neutral: NEUTRAL,
    satellites: &[("de-DE", DE_DE), ("fr", FR), ("fr-CA", FR_CA)],
    files: &[("logo.png", b"EN-LOGO"), ("logo.fr.png", b"FR-LOGO")],
};
static SET: StaticSet = StaticSet::new(&EMBEDDED);

#[test]
fn culture_fallback_specific_neutral_sibling_invariant() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    assert_eq!(SET.text_in("hello", "fr-CA"), "Allô");
    assert_eq!(SET.text_in("bye", "fr-CA"), "Au revoir", "fr-CA → fr");
    assert_eq!(SET.text_in("hello", "fr-FR"), "Bonjour", "fr-FR → fr");
    assert_eq!(SET.text_in("only_neutral", "fr-CA"), "Neutral", "→ neutral file");
    assert_eq!(SET.text_in("hello", "de-AT"), "Hallo", "de-AT → de-DE (same language)");
    assert_eq!(SET.text_in("hello", "ja-JP"), "Hello");
    assert_eq!(SET.text_in("hello", "invariant"), "Hello");
    set_culture("fr-BE");
    assert_eq!(SET.text("hello"), "Bonjour");
    assert_eq!(SET.bytes("logo").0, b"FR-LOGO");
    set_culture("en-US");
    assert_eq!(SET.bytes("logo"), (&b"EN-LOGO"[..], "png"));
    assert_eq!(SET.bytes("dot"), (&[0u8, 1, 2][..], "png"));
    assert_eq!(SET.text("accent"), "#3366FF");
}

#[test]
fn lookups_and_uris_follow_the_culture() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    register_static(&SET);
    register_static(&SET); // Idempotent.
    assert_eq!(set_names().iter().filter(|n| *n == "testres").count(), 1);
    set_culture("fr");
    assert_eq!(string("hello"), "Bonjour");
    let (id_fr, bytes, format) = resolve_uri("kbres:testres/logo").expect("image");
    assert_eq!(&*bytes, b"FR-LOGO");
    assert_eq!(format, "png");
    set_culture("en");
    let (id_en, bytes, _) = resolve_uri("kbres:logo").expect("image without set");
    assert_eq!(&*bytes, b"EN-LOGO");
    assert_ne!(id_fr, id_en, "a culture switch changes the content id the image caches key by");
    assert!(resolve_uri("kbres:testres/hello").is_none(), "a string is not an image");
    assert!(lookup(None, Some("nope"), "hello").is_none());
    assert_eq!(parse_uri("kbres:a/b"), Some((Some("a"), "b")));
    assert_eq!(parse_uri("images/x.png"), None);
}

#[test]
fn loaded_sets_hide_embedded_ones_and_read_linked_files() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    register_static(&SET);
    let dir = std::env::temp_dir().join(format!("kbres-loaded-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    std::fs::write(dir.join("logo.png"), b"DISK-LOGO").expect("write");
    let loaded = LoadedSet::from_texts("testres", &dir, r#"<Resources><String Name="hello">Hi (designer)</String><Image Name="logo" File="logo.png"/><Image Name="e" Format="png">AAEC</Image></Resources>"#, &[("fr".into(), r#"<Resources><String Name="hello">Salut (designer)</String></Resources>"#.into())]);
    replace_loaded(vec![Arc::new(loaded)]);
    set_culture("fr-FR");
    assert_eq!(string("hello"), "Salut (designer)");
    assert_eq!(&*resolve_uri("kbres:testres/logo").expect("logo").1, b"DISK-LOGO");
    let a = resolve_uri("kbres:testres/e").expect("embedded").0;
    let b = resolve_uri("kbres:testres/e").expect("embedded").0;
    assert_eq!(a, b, "embedded bytes of a loaded set keep one identity");
    replace_loaded(Vec::new());
    assert_eq!(string("hello"), "Bonjour");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn culture_changes_are_announced() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    set_culture("en-US");
    let before = generation();
    let sub = on_culture_changed(|c| {
        assert_eq!(c, "de-DE");
        CALLS.fetch_add(1, Ordering::SeqCst);
    });
    set_culture("de-de");
    assert_eq!(culture(), "de-DE");
    set_culture("de-DE"); // Unchanged: no notification.
    assert_eq!(CALLS.load(Ordering::SeqCst), 1);
    assert!(generation() > before);
    drop(sub);
    set_culture("en-US");
    assert_eq!(CALLS.load(Ordering::SeqCst), 1, "a dropped subscription is not called");
}
