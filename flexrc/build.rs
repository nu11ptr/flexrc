fn main() {
    println!("cargo:rustc-check-cfg=cfg(flexrc_sanitize_thread)");

    let sanitize = std::env::var("CARGO_CFG_SANITIZE").unwrap_or_default();
    if sanitize.split(',').any(|value| value == "thread") {
        println!("cargo:rustc-cfg=flexrc_sanitize_thread");
    }
}
