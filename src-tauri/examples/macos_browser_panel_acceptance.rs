//! Standalone native acceptance host. It links the production panel modules but
//! never initializes CrewVela account, session, provider, or application data.
//! Usage: cargo run --example macos_browser_panel_acceptance -- /absolute/empty-dir
//! A browser-control MCP client may connect after the `READY <directory>` line.

#[cfg(target_os = "macos")]
#[path = "../src/browser_panel.rs"]
mod browser_panel;
#[cfg(target_os = "macos")]
#[path = "../src/browser_panel_macos.rs"]
mod browser_panel_macos;

#[cfg(target_os = "macos")]
static ACCEPTANCE_ROOT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

#[cfg(target_os = "macos")]
mod providers {
    pub fn config_dir() -> anyhow::Result<std::path::PathBuf> {
        super::ACCEPTANCE_ROOT.get().cloned()
            .ok_or_else(|| anyhow::anyhow!("验收临时配置目录尚未初始化"))
    }
}

#[cfg(target_os = "macos")]
mod browser_control {
    pub const PANEL_NEEDED_EVENT: &str = "browser-control-panel-needed";
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct BrowserControlSettings {
        pub enabled: bool,
        pub prefer_panel: bool,
    }
}

#[cfg(target_os = "macos")]
mod test_profile {
    // This is an independent test executable with a new config directory; it
    // does not call the production isolation mode or weaken its deny guard.
    pub fn active() -> bool { false }
}

#[cfg(target_os = "macos")]
fn main() -> anyhow::Result<()> {
    use anyhow::{bail, Context};
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    use tauri::{Listener, Manager};

    let requested = std::env::args_os().nth(1).map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join(format!("crewvela-panel-acceptance-{}", uuid::Uuid::new_v4())));
    if !requested.is_absolute() { bail!("验收目录必须是绝对路径"); }
    std::fs::create_dir_all(&requested)?;
    if std::fs::read_dir(&requested)?.next().is_some() {
        bail!("验收目录必须为空，拒绝覆盖已有应用配置");
    }
    std::fs::set_permissions(&requested, std::fs::Permissions::from_mode(0o700))?;
    let root = requested.canonicalize()?;
    let mut settings = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600)
        .open(root.join("browser-control.json"))?;
    settings.write_all(br#"{"enabled":true,"preferPanel":true}"#)?;
    ACCEPTANCE_ROOT.set(root.clone()).map_err(|_| anyhow::anyhow!("验收目录重复初始化"))?;

    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    context.config_mut().identifier = "com.crewvela.browser-panel-acceptance".into();
    context.config_mut().product_name = Some("CrewVela Panel Acceptance".into());
    tauri::Builder::default()
        .setup(move |app| {
            // A native host window, not CrewVela's main frontend. Remote content
            // only ever loads in the production browser-panel child WebView.
            tauri::WindowBuilder::new(app, "main")
                .title("CrewVela macOS Browser Panel Acceptance")
                .inner_size(1200.0, 800.0)
                .visible(true)
                .build()?;
            let handle = app.handle().clone();
            app.listen(browser_control::PANEL_NEEDED_EVENT, move |_| {
                let visible_app = handle.clone();
                // Production delegates show/hide to React. This dedicated
                // host has no React frontend, so mimic its post-event show
                // asynchronously, never waiting on the event-dispatch thread.
                let _ = handle.run_on_main_thread(move || {
                    if let Some(view) = visible_app.get_webview(browser_panel::BROWSER_PANEL_LABEL) {
                        if let Err(error) = view.show() {
                            eprintln!("验收面板显示失败：{error}");
                        }
                    }
                });
            });
            browser_panel_macos::start_service(app.handle(), &root)?;
            println!("READY {}", root.display());
            std::io::stdout().flush()?;
            Ok(())
        })
        .run(context).context("原生浏览器面板验收宿主运行失败")?;
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("This acceptance host requires macOS.");
    std::process::exit(2);
}
