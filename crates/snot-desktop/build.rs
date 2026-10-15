fn main() {
    // On Windows, link in the icon from assets/windows/snot.rc. A build script
    // is compiled for the host, so ask Cargo about the target instead of
    // using cfg!.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=assets/windows/snot.rc");
        println!("cargo:rerun-if-changed=assets/windows/snot.ico");
        embed_resource::compile("assets/windows/snot.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap();
    }
}
