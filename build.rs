fn main() {
    cxx_build::bridge("src/bridge.rs")
        .std("c++20")
        .compile("ron_browser_bridge");
    println!("cargo:rerun-if-changed=src/bridge.rs");
}
