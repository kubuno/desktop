//! The element tables of `#[kubuno::view]` (in `kubuno-views-meta`, which the macro reads) agree with
//! the registry of `kubuno-views` and with the typed handles of this crate.

use kubuno_views_meta::kbview::{BUILTIN_ELEMENTS, PRINT_ELEMENTS, PRINT_TYPED, TYPED_CONTROLS};

#[test]
fn the_builtin_elements_are_the_registry() {
    // The registry also holds the classes of the libraries this crate links (`kubuno-print`, and
    // `kubuno-data` with the `data` feature): those are in `LIBRARY_ELEMENTS`, not built in.
    let mut registry: Vec<&str> = kubuno::views::registry::all()
        .iter()
        .filter(|c| c.name != "Application" && kubuno::views::registry::project_info(c.name).is_none())
        .map(|c| c.name)
        .collect();
    registry.sort_unstable();
    let mut table = BUILTIN_ELEMENTS.to_vec();
    table.sort_unstable();
    assert_eq!(table, registry);
}

#[test]
fn every_typed_control_has_a_handle_of_its_name() {
    for name in TYPED_CONTROLS {
        let control = kubuno::Control::new(name);
        assert!(kubuno::views::registry::lookup(name).is_some(), "{name} is not in the registry");
        let handle = kubuno::__private::typed_handle_element(name);
        assert_eq!(handle, Some(*name), "no `kubuno::forms::{name}`");
        drop(control);
    }
}

#[test]
fn the_printing_classes_are_linked_and_have_their_handles() {
    // Linked through `kubuno::printing` (no `extern crate` needed in the application).
    for name in PRINT_ELEMENTS {
        let info = kubuno::views::registry::project_info(name);
        assert!(info.is_some_and(|i| i.crate_name == Some("kubuno_print")), "{name} is not registered by kubuno_print");
    }
    let handles = [
        kubuno::printing::PrintDocument::ELEMENT,
        kubuno::printing::PrintPreviewDialog::ELEMENT,
        kubuno::printing::PrintDialog::ELEMENT,
        kubuno::printing::PageSetupDialog::ELEMENT,
        kubuno::printing::PrintPreviewControl::ELEMENT,
    ];
    let mut handles = handles.to_vec();
    handles.sort_unstable();
    assert_eq!(handles, PRINT_TYPED);
}

#[test]
fn the_storage_classes_are_linked_and_have_their_handles() {
    use kubuno_views_meta::kbview::{STORAGE_ELEMENTS, STORAGE_TYPED};
    // Linked through `kubuno::storage` (no `extern crate` needed in the application).
    for name in STORAGE_ELEMENTS {
        let info = kubuno::views::registry::project_info(name);
        assert!(info.is_some_and(|i| i.crate_name == Some("kubuno_app_storage_components")), "{name} is not registered by kubuno_app_storage_components");
    }
    let mut handles = vec![
        kubuno::storage::Settings::ELEMENT,
        kubuno::storage::SecretStore::ELEMENT,
        kubuno::storage::RegistryKey::ELEMENT,
        kubuno::storage::KeyValueStore::ELEMENT,
        kubuno::storage::FileStore::ELEMENT,
    ];
    handles.sort_unstable();
    assert_eq!(handles, STORAGE_TYPED);
}
