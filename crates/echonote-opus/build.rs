//! Compile the vendored libopus with `cc` (no CMake). The source list and headers come from
//! `scripts/dev/vendor_opus.py`. Plain C only: at 16 kHz mono the encoder costs well under
//! 1% of a core without SIMD, and leaving out intrinsics keeps one configuration for every
//! target.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=opus");
    println!("cargo:rerun-if-env-changed=DOCS_RS");
    // Cross-target type checks (DOCS_RS=1, as for sherpa-onnx) have no C toolchain for the
    // target; nothing links in a check build, so skip compiling.
    if std::env::var_os("DOCS_RS").is_some() {
        return;
    }

    let root = Path::new("opus");
    let sources = std::fs::read_to_string(root.join("sources.txt")).expect("opus/sources.txt");
    let version = std::fs::read_to_string(root.join("package_version"))
        .expect("opus/package_version")
        .trim()
        .trim_start_matches("PACKAGE_VERSION=")
        .to_owned();
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");

    let mut build = cc::Build::new();
    build
        .include(root.join("include"))
        .include(root.join("celt"))
        .include(root.join("silk"))
        .include(root.join("silk/float"))
        .include(root.join("src"))
        .define("OPUS_BUILD", None)
        .define("PACKAGE_VERSION", version.as_str())
        .define("HAVE_LRINT", None)
        .define("HAVE_LRINTF", None)
        .define("ENABLE_HARDENING", None)
        .define("DISABLE_DEBUG_FLOAT", None)
        // MSVC has no variable-length arrays.
        .define(if msvc { "USE_ALLOCA" } else { "VAR_ARRAYS" }, None)
        .warnings(false);
    if !msvc {
        build.flag_if_supported("-fstack-protector-strong");
    }
    for source in sources.lines().filter(|line| !line.is_empty()) {
        build.file(root.join(source));
    }
    build.compile("opus");
}
