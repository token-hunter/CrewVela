fn main() {
    println!("cargo:rerun-if-env-changed=CREWVELA_MEMORY_ARCHIVE");
    println!("cargo:rustc-check-cfg=cfg(crewvela_embedded_memory)");
    if let Some(archive) = std::env::var_os("CREWVELA_MEMORY_ARCHIVE") {
        let archive = std::path::PathBuf::from(archive);
        assert!(archive.is_absolute() && archive.is_file(), "内置共享记忆安装包不可读");
        println!("cargo:rerun-if-changed={}", archive.display());
        println!("cargo:rustc-env=CREWVELA_MEMORY_ARCHIVE={}", archive.display());
        println!("cargo:rustc-cfg=crewvela_embedded_memory");
    }
    // A release profile alone does not enable Tauri's embedded asset protocol.
    // Refuse binaries that would still navigate to the Vite dev server.
    if std::env::var("PROFILE").as_deref() == Ok("release") && tauri_build::is_dev() {
        panic!("发布构建缺少 custom-protocol，会依赖 localhost 开发服务器。请使用 npm run build:desktop，或 cargo build --release --features custom-protocol");
    }
    // Emit Tauri's default controls-v6 dependency through the MSVC linker so
    // both the application and library tests receive it. Keep it out of the
    // binary-only resource to avoid embedding two manifests in the application.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows")
        && std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc")
    {
        tauri_build::try_build(tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        ))
        .expect("failed to build Tauri resources");
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'");
    } else {
        tauri_build::build();
    }
}
