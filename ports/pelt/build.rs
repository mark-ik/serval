fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        // Scripted documents (Boa heap, DOM reflectors, and the Livery session)
        // need more than the Windows process-main stack reserved by the default
        // Rust link. The winit event loop must stay on that main thread.
        println!("cargo:rustc-link-arg-bin=pelt=/STACK:8388608");
    }
}
