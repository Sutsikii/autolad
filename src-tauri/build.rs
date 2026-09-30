use tauri_build::{Attributes, WindowsAttributes};

fn main() {
    // Tauri's default manifest is only embedded in the app binary, so unit-test binaries abort
    // at load time (STATUS_ENTRYPOINT_NOT_FOUND) for lack of comctl32 v6. Embed our own manifest
    // for every target instead.
    let attrs = Attributes::new().windows_attributes(WindowsAttributes::new_without_app_manifest());
    if let Err(e) = tauri_build::try_build(attrs) {
        panic!("tauri build script failed: {e:#}");
    }

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
        let manifest = std::path::Path::new(&dir).join("app.manifest");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rerun-if-changed=app.manifest");
    }
}
