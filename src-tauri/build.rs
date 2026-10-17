fn main() {
    // tauri-build embeds icons/icon.ico but does not watch it, so a new icon would keep the old one.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
