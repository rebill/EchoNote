fn main() {
    println!("cargo:rerun-if-changed=native/macos_tap.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/macos_tap.m")
            .flag("-fobjc-arc")
            .compile("echonote_macos_tap");
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }
}
