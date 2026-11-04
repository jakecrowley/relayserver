extern crate cc;
fn main() {
    // Device (C bindings, MobileGestalt) only builds for the iOS target.
    // Host builds skip it so pure-Rust unit tests can run via `cargo test --lib`.
    let is_ios = std::env::var("CARGO_CFG_TARGET_OS").map(|os| os == "ios").unwrap_or(false);
    if !is_ios {
        return;
    }
    let theos = std::env::var("THEOS").unwrap_or_else(|_| "/home/jake/theos".to_string());
    let sdk = format!("{}/sdks/iPhoneOS16.5.sdk", theos);

    println!("cargo:rustc-link-lib=dylib=MobileGestalt");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");

    cc::Build::new()
        .compiler(format!("{}/toolchain/linux/iphone/bin/clang", theos))
        .archiver(format!("{}/toolchain/linux/iphone/bin/ar", theos))
        .include("src/c")
        .flag("-isysroot")
        .flag(&sdk)
        .flag(format!("-F{}/System/Library/Frameworks", sdk))
        .flag("-target")
        .flag("arm64-apple-ios14.0")
        .file("src/c/relay.c")
        .file("src/c/absdUser.c")
        .compile("relay");
}
