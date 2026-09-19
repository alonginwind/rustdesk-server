use cc::Build;

fn main() {
    println!("cargo:rerun-if-changed=src");
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os == "windows" {
        println!("cargo:rustc-link-lib=kernel32");
        Build::new().file("src/win.cpp").compile("machine-uid");
    }
}
