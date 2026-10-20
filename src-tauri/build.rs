fn main() {
    // tauri-build declares its own rerun triggers, which turns off Cargo's default
    // "rerun on any change", so icon edits would otherwise keep the old icon in the exe.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
