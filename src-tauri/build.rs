fn main() {
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
