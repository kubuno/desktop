//! A program linked against `kubuno-ui` loads a DLL named after its build (`build.rs`): this test
//! program is one, so the module holding the library's code must be `kubuno_ui-<hash>.dll`.

#[cfg(all(windows, target_env = "msvc"))]
#[test]
fn the_loaded_library_is_named_after_its_build() {
    let path = kubuno_ui::library::module_path().expect("the library's module is known");
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
    assert!(kubuno_ui::library::is_library_file_name(&name), "{}", path.display());
    assert_ne!(
        name.to_ascii_lowercase(),
        "kubuno_ui.dll",
        "the link shim did not rename the DLL (loaded {})",
        path.display()
    );
}
