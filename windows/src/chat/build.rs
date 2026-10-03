fn main() {
    // DPI awareness + common controls v6, like the Drive app and the shell.
    embed_manifest::embed_manifest(embed_manifest::new_manifest("Kubuno.Chat"))
        .expect("unable to embed application manifest");
    // The app icon, under the name the window class loads it by.
    let mut res = winresource::WindowsResource::new();
    res.set_icon_with_id("assets/chat.ico", "app_icon");
    res.compile().expect("unable to embed icon resource");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/chat.ico");
}
