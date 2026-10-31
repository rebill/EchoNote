fn main() {
    println!("cargo:rerun-if-changed=native/macos_tap.m");
    println!("cargo:rerun-if-changed=native/macos_system.m");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/macos_tap.m")
            .file("native/macos_system.m")
            .flag("-fobjc-arc")
            .compile("echonote_macos");
        println!("cargo:rustc-link-lib=framework=AVFoundation");
        println!("cargo:rustc-link-lib=framework=CoreAudio");
        println!("cargo:rustc-link-lib=framework=Foundation");
    }
}
