fn main() {
    println!(
        "cargo:rustc-env=PICKET_TARGET={}",
        std::env::var("TARGET").unwrap()
    );
    tauri_build::build();
}
