use std::path::Path;

fn main() {
    bundle_wasm_backends();
    tauri_build::build()
}

/// Copy the local-first WASM backends (documents-core / drive-core) next to the
/// executable when their build artifacts are available in the workspace
/// (`_artifacts/`), so the local-first path is active by default. If the
/// artifacts are absent (e.g. a standalone build outside the workspace), nothing
/// is copied and the app falls back to the core proxy + cache.
fn bundle_wasm_backends() {
    let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    // CARGO_MANIFEST_DIR = <workspace>/desktop/app/src-tauri → up 3 = <workspace>
    let artifacts = Path::new(&manifest).join("../../../_artifacts");

    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return;
    };
    // OUT_DIR = <target>/<profile>/build/<pkg>-<hash>/out → up 3 = <target>/<profile>
    let Some(exe_dir) = Path::new(&out_dir).ancestors().nth(3) else {
        return;
    };

    for name in ["documents-core.wasm", "drive-core.wasm"] {
        let src = artifacts.join(name);
        println!("cargo:rerun-if-changed={}", src.display());
        if src.is_file() {
            let _ = std::fs::copy(&src, exe_dir.join(name));
        }
    }
}
