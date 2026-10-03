fn main() {
    embed_manifest::embed_manifest(embed_manifest::new_manifest("Kubuno.Drive"))
        .expect("unable to embed application manifest");
    // Embed the Drive web logo (drive-logo.svg → drive.ico) as the executable's
    // icon: Explorer, taskbar and Alt-Tab all read it from the exe resources.
    // The window class loads the same resource by name ("app_icon").
    let mut res = winresource::WindowsResource::new();
    res.set_icon_with_id("assets/drive.ico", "app_icon");
    res.compile().expect("unable to embed icon resource");
    // libgit2 (vendored) references advapi32: security tokens (OpenProcessToken,
    // GetNamedSecurityInfoW) and the legacy Crypt* API (CryptGenRandom). Without
    // `git2`'s transport features, `libgit2-sys` does not emit this link.
    println!("cargo:rustc-link-lib=dylib=advapi32");
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/drive.ico");
}
