//! Plural keys through `resources!`: `files(count)` for a key that only exists as plural forms, satellite
//! forms the neutral language lacks (Russian `_few` / `_many`), and `text_with` (what `{Res}` arguments use).

kubuno_desktop_resources::resources!("fixtures/plural.kbres");

#[test]
fn plural_accessors_and_arguments() {
    assert_eq!(Plural::NAMES, &["files_zero", "files_one", "files_other", "shared_by"]);
    let set = Plural::set();
    assert_eq!(set.plural_in("files", 0.0, "en"), "No files");
    assert_eq!(set.plural_in("files", 1.0, "en"), "1 file");
    assert_eq!(set.plural_in("files", 2.5, "en"), "2.5 files");
    assert_eq!(set.plural_in("files", 1.0, "ru"), "1 файл");
    assert_eq!(set.plural_in("files", 3.0, "ru"), "3 файла");
    assert_eq!(set.plural_in("files", 5.0, "ru-RU"), "5 файлов", "a form only the satellite has");
    assert_eq!(set.plural_in("files", 0.0, "ru"), "0 файлов", "i18next: the Russian form wins over the neutral file's files_zero");
    assert_eq!(set.plural_in("files", 0.0, "fr"), "No files", "no French file: the neutral files_zero");
    assert_eq!(set.plural_in("nope", 1.0, "en"), "");
    Plural::set_culture("en");
    assert_eq!(Plural::files(2.0), "2 files");
    let args = vec![("Name".to_string(), "Ana".to_string()), ("Sep".to_string(), ", ".to_string()), ("Count".to_string(), "3".to_string())];
    assert_eq!(kubuno_desktop_resources::text_with_in(Some("plural"), "shared_by", None, &args, "en").as_deref(), Some("Shared by Ana, 3 items"));
    assert_eq!(kubuno_desktop_resources::text_with_in(Some("plural"), "files", Some(1.0), &args, "en").as_deref(), Some("3 file"), "{{count}} is the Count argument's text");
    assert_eq!(kubuno_desktop_resources::text_with_in(Some("plural"), "files", Some(7.0), &[], "ru").as_deref(), Some("{{count}} файлов"));
    assert_eq!(kubuno_desktop_resources::text_with_in(Some("plural"), "nope", Some(1.0), &args, "en"), None);
}
