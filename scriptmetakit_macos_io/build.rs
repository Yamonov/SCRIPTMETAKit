fn main() {
    println!("cargo:rerun-if-changed=src/dispatch_io_shim.c");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }

    cc::Build::new()
        .file("src/dispatch_io_shim.c")
        .flag("-fblocks")
        .warnings(true)
        .compile("scriptmetakit_dispatch_io");
    println!("cargo:rustc-link-lib=System");
}
