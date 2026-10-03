//! Pins `examples/views/showcase.kbview` — the family-integration visual
//! check (`examples/showcase.rs`) — to actually compiling against the
//! default registry: a typo in an element/attribute/enum name there would
//! otherwise only surface by running the example and reading the on-screen
//! diagnostics banner. Requires every family feature (this crate's own
//! `all-features`, the default since the families were integrated) —
//! skipped harmlessly under `--no-default-features`.

#[cfg(feature = "all-families")]
#[test]
fn showcase_kbview_compiles_against_the_default_registry() {
    let src = include_str!("../examples/views/showcase.kbview");
    match kubuno_desktop_views::compile::compile(src) {
        Ok(_) => {}
        Err(diags) => panic!("showcase.kbview failed to compile:\n{diags:#?}"),
    }
}
