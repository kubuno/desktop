//! The real `resources!` expansion: typed accessors, culture switching, registration before `main`.

kubuno_resources::resources!("fixtures/app.kbres");
kubuno_resources::resources!(pub(crate) Other, "fixtures/app.kbres");

#[test]
fn generated_accessors_follow_the_culture() {
    // Registered before `main` (the `.CRT$XCU` initializer), before any accessor ran.
    assert!(kubuno_resources::set_names().iter().any(|n| n == "app"));
    assert_eq!(App::NAMES, &["welcome_text", "OkButton.Text", "flag", "accent", "heading", "notes"]);
    App::set_culture("fr-FR");
    assert_eq!(App::welcome_text(), "Bienvenue");
    assert_eq!(App::ok_button_text(), "OK", "not translated: neutral value");
    assert_eq!(App::flag().bytes(), b"FR");
    assert_eq!(App::flag().uri(), "kbres:app/flag");
    assert_eq!(kubuno_resources::string("welcome_text"), "Bienvenue");
    App::set_culture("en-GB");
    assert_eq!(App::welcome_text(), "Welcome");
    assert_eq!(App::flag().bytes(), b"EN");
    assert_eq!(App::flag().format(), "png");
    assert_eq!(App::accent().to_hex(), "#3366FF");
    assert_eq!(App::heading().as_str(), "Segoe UI, 14pt, style=Bold");
    assert_eq!(App::notes(), "hello");
    assert_eq!(Other::welcome_text(), "Welcome");
    assert!(matches!(App::get("welcome_text"), Some(kubuno_resources::ResolvedValue::Text { .. })));
}
