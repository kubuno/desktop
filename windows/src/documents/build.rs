fn main() {
    // DPI awareness + common controls v6, like the Drive app, the shell and chat.
    embed_manifest::embed_manifest(embed_manifest::new_manifest("Kubuno.Documents"))
        .expect("unable to embed application manifest");
    // The app icon, under the name the window class loads it by. It is built
    // from the module's own web logo (core/frontend/public/office-documents-logo.png),
    // never the generic Kubuno mark.
    let mut res = winresource::WindowsResource::new();
    res.set_icon_with_id("assets/documents.ico", "app_icon");
    res.compile().expect("unable to embed icon resource");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/documents.ico");
}
