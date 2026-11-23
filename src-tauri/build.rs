fn main() {
    tauri_build::build();
    // Tauri embeds the production manifest for binaries, not Cargo examples.
    // The real-window regression also needs named comctl32 v6 exports.
    #[cfg(windows)]
    {
        let resources =
            std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("resource.lib");
        println!("cargo:rustc-link-arg-examples={}", resources.display());
    }
}
