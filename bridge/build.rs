fn main() {
    cxx_build::bridge("src/lib.rs")
        .std("c++20")
        .include("include")
        .compile("ron_browser_bridge_glue");
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=include/text_measure.h");
}
