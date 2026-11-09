//! Opt-in native WebView2 host for the isolated settings-layout fixture.
//! It deliberately does not run the application's setup or register business state.

use tauri::{Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

#[test]
#[ignore = "manual Windows WebView2 settings check; requires local Vite and CREWVELA_RUN_NATIVE_SETTINGS_TEST=1"]
fn native_settings_frame_without_user_data_or_model_calls() {
    assert_eq!(
        std::env::var("CREWVELA_RUN_NATIVE_SETTINGS_TEST").as_deref(),
        Ok("1"),
        "Explicitly opt in to the isolated native settings window",
    );
    let temporary_parent = std::env::temp_dir().canonicalize().expect("解析系统临时目录失败");
    let root = temporary_parent.join(format!("crewvela-native-settings-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).expect("创建原生设置验收临时目录失败");
    let data_directory = root.join("webview2");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.crewvela.settings-frame-test".into();
    context.config_mut().build.dev_url = Some("http://127.0.0.1:1420".parse().unwrap());
    eprintln!("[native-settings] isolated-data={} pid={}", root.display(), std::process::id());

    let app = tauri::Builder::default()
        .any_thread()
        .setup(move |app| {
            let url = "http://127.0.0.1:1420/tests/fixtures/crewvela-settings-frame-ui.html?ui-preview=1&archive=long";
            let window = WebviewWindowBuilder::new(app, "settings-frame-native-test", WebviewUrl::External(url.parse()?))
                .title("CrewVela 设置尺寸验收")
                .inner_size(1440.0, 900.0)
                .decorations(true)
                .resizable(true)
                .data_directory(data_directory.clone())
                .on_page_load(|window, payload| {
                    eprintln!("[native-settings] page-load={:?} outer={:?} inner={:?} scale={:?}",
                        payload.event(), window.outer_size(), window.inner_size(), window.scale_factor());
                })
                .build()?;
            eprintln!("[native-settings] ready pid={} hwnd={:?} outer={:?} inner={:?} scale={:?}",
                std::process::id(), window.hwnd(), window.outer_size(), window.inner_size(), window.scale_factor());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Resized(size) => {
                eprintln!("[native-settings] resized inner={}x{} outer={:?}", size.width, size.height, window.outer_size());
            }
            WindowEvent::ScaleFactorChanged { scale_factor, new_inner_size, .. } => {
                eprintln!("[native-settings] scale-factor={} inner={}x{} outer={:?}",
                    scale_factor, new_inner_size.width, new_inner_size.height, window.outer_size());
            }
            WindowEvent::CloseRequested { .. } => {
                eprintln!("[native-settings] close-requested outer={:?}", window.outer_size());
                window.app_handle().exit(0);
            }
            _ => {}
        })
        .build(context).expect("原生设置验收窗口创建失败");
    let exit_code = app.run_return(|_, _| {});

    let cleanup_root = root.canonicalize().expect("解析原生验收清理目录失败");
    assert_eq!(cleanup_root.parent(), Some(temporary_parent.as_path()), "拒绝清理隔离目录以外的路径");
    let cleanup_deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match std::fs::remove_dir_all(&cleanup_root) {
            Ok(()) => break,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            // WebView2 releases profile files after the native event loop ends.
            // Retry sharing violations only; permission/policy failures stay visible.
            Err(error) if error.raw_os_error() == Some(32) && std::time::Instant::now() < cleanup_deadline => {
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(error) => panic!("原生设置验收已退出，但临时目录清理失败（保留供检查）：{}：{error}", root.display()),
        }
    }
    eprintln!("[native-settings] isolated-data-cleaned={}", root.display());
    assert_eq!(exit_code, 0, "原生设置验收窗口异常退出");
}
