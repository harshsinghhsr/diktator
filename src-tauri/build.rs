fn main() {
    // llama.cpp's Metal backend uses `@available`, which calls
    // `__isPlatformVersionAtLeast` from clang's runtime. rustc links with
    // -nodefaultlibs, so add that runtime back or release builds fail to link.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let out = std::process::Command::new("clang")
            .arg("--print-runtime-dir")
            .output()
            .expect("clang not found");
        let dir = String::from_utf8(out.stdout).unwrap();
        println!("cargo:rustc-link-search=native={}", dir.trim());
        println!("cargo:rustc-link-lib=static=clang_rt.osx");
    }
    tauri_build::build()
}
