fn main() {
    // Local development dependencies found by pkg-config also need to be
    // discoverable when running the debug executable directly.
    let native_libs = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".native/usr/lib64");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
        && std::env::var("PROFILE").as_deref() == Ok("debug")
        && native_libs.is_dir()
    {
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", native_libs.display());
    }
    glib_build_tools::compile_resources(
        &["resources"],
        "resources/resources.gresource.xml",
        "brevlada.gresource",
    );
}
