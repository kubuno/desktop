//! The element tables of `#[kubuno_desktop::view]` (in `kubuno-desktop-views-meta`, which the macro reads) agree with
//! the registry of `kubuno-desktop-views` and with the typed handles of this crate.

use kubuno_desktop_views_meta::kbview::{BUILTIN_ELEMENTS, PRINT_ELEMENTS, PRINT_TYPED, TYPED_CONTROLS};

#[test]
fn the_builtin_elements_are_the_registry() {
    // The registry also holds the classes of the libraries this crate links (`kubuno-desktop-print`, and
    // `kubuno-desktop-data` with the `data` feature): those are in `LIBRARY_ELEMENTS`, not built in.
    let mut registry: Vec<&str> = kubuno_desktop::views::registry::all()
        .iter()
        .filter(|c| c.name != "Application" && kubuno_desktop::views::registry::project_info(c.name).is_none())
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
        let control = kubuno_desktop::Control::new(name);
        assert!(kubuno_desktop::views::registry::lookup(name).is_some(), "{name} is not in the registry");
        let handle = kubuno_desktop::__private::typed_handle_element(name);
        assert_eq!(handle, Some(*name), "no `kubuno_desktop::forms::{name}`");
        drop(control);
    }
}

#[test]
fn the_printing_classes_are_linked_and_have_their_handles() {
    // Linked through `kubuno_desktop::printing` (no `extern crate` needed in the application).
    for name in PRINT_ELEMENTS {
        let info = kubuno_desktop::views::registry::project_info(name);
        assert!(info.is_some_and(|i| i.crate_name == Some("kubuno_desktop_print")), "{name} is not registered by kubuno_desktop_print");
    }
    let handles = [
        kubuno_desktop::printing::PrintDocument::ELEMENT,
        kubuno_desktop::printing::PrintPreviewDialog::ELEMENT,
        kubuno_desktop::printing::PrintDialog::ELEMENT,
        kubuno_desktop::printing::PageSetupDialog::ELEMENT,
        kubuno_desktop::printing::PrintPreviewControl::ELEMENT,
    ];
    let mut handles = handles.to_vec();
    handles.sort_unstable();
    assert_eq!(handles, PRINT_TYPED);
}

#[test]
fn the_storage_classes_are_linked_and_have_their_handles() {
    use kubuno_desktop_views_meta::kbview::{STORAGE_ELEMENTS, STORAGE_TYPED};
    // Linked through `kubuno_desktop::storage` (no `extern crate` needed in the application).
    for name in STORAGE_ELEMENTS {
        let info = kubuno_desktop::views::registry::project_info(name);
        assert!(info.is_some_and(|i| i.crate_name == Some("kubuno_desktop_app_storage_components")), "{name} is not registered by kubuno_desktop_app_storage_components");
    }
    let mut handles = vec![
        kubuno_desktop::storage::Settings::ELEMENT,
        kubuno_desktop::storage::SecretStore::ELEMENT,
        kubuno_desktop::storage::RegistryKey::ELEMENT,
        kubuno_desktop::storage::KeyValueStore::ELEMENT,
        kubuno_desktop::storage::FileStore::ELEMENT,
    ];
    handles.sort_unstable();
    assert_eq!(handles, STORAGE_TYPED);
}
