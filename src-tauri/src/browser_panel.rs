//! 右侧栏浏览器面板：主窗口内嵌原生子 webview（Windows 为 WebView2 内核）。
//! 生命周期：首次 open 按传入矩形创建并常驻到应用退出，切走标签页/关栏只
//! hide/show，尺寸变化由前端 ResizeObserver 驱动 set_bounds（逻辑坐标，Tauri
//! 按 scale factor 自动换算物理像素）。
//! 安全：子 webview 的 label 不进入任何 capability，远程页面拿不到 Tauri IPC。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use tauri::{
    webview::PageLoadEvent, AppHandle, Emitter, LogicalPosition, LogicalSize, Manager, Position,
    Rect, Runtime, Size, Webview, WebviewBuilder, WebviewUrl,
};

pub const BROWSER_PANEL_LABEL: &str = "browser-panel";
const BROWSER_PANEL_NAVIGATED_EVENT: &str = "browser-panel-navigated";
const BROWSER_PANEL_LOAD_EVENT: &str = "browser-panel-load";

// open 和 browser-control 的重建共用一把锁；检查 label 到发布 CDP 文件必须原子。
// 仅后台生命周期入口取锁，UI 主线程上的显隐/尺寸命令不能等锁，否则 add_child
// 等待主线程创建 WebView 时会死锁。
static PANEL_LIFECYCLE: Mutex<()> = Mutex::new(());
// 控制器损坏后不能再读取 COM bounds；保留前端最后成功提交的逻辑矩形。
static LAST_PANEL_BOUNDS: Mutex<Option<(f64, f64, f64, f64)>> = Mutex::new(None);

// 本实例实际创建的独立端点：恢复时绝不信任可能指向其它浏览器的端口文件。
#[cfg(target_os = "windows")]
static OWNED_PANEL_CDP_PORT: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

fn panel_lifecycle_lock() -> Result<MutexGuard<'static, ()>, String> {
    PANEL_LIFECYCLE
        .lock()
        .map_err(|_| "浏览器面板生命周期锁已损坏".to_string())
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct NavigatedPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    panel_id: Option<String>,
    url: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct LoadPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    panel_id: Option<String>,
    event: String,
    url: String,
}

/// 面板只放行 http/https；javascript: 之类会在 WebView2 里以特权执行，绝不接受。
pub fn validate_url(raw: &str) -> Result<tauri::Url, String> {
    let url = raw
        .parse::<tauri::Url>()
        .map_err(|_| format!("无法解析网址: {raw}"))?;
    match url.scheme() {
        "http" | "https" if !matches!(url.host_str(), Some("tauri.localhost" | "asset.localhost" | "ipc.localhost")) => Ok(url),
        "http" | "https" => Err("浏览器面板不能打开应用内部地址".into()),
        scheme => Err(format!(
            "浏览器面板仅支持 http/https 网址（当前 {scheme}）: {raw}"
        )),
    }
}

fn validate_panel_url<R: Runtime>(app: &AppHandle<R>, raw: &str) -> Result<tauri::Url, String> {
    let url = validate_url(raw)?;
    if cfg!(debug_assertions) && app.config().build.dev_url.as_ref()
        .is_some_and(|development| development.origin() == url.origin()) {
        return Err("浏览器面板不能打开应用自身的开发服务地址".into());
    }
    Ok(url)
}

/// wry 的 WebView2 后端没有原生 GoBack/GoForward，后退/前进/刷新走页面脚本。
pub fn history_script(action: &str) -> Result<&'static str, String> {
    match action {
        "back" => Ok("history.back()"),
        "forward" => Ok("history.forward()"),
        "reload" => Ok("location.reload()"),
        other => Err(format!("未知的浏览器导航动作: {other}")),
    }
}

pub fn panel_exists<R: tauri::Runtime>(app: &AppHandle<R>) -> bool {
    panel(app).is_some()
}

pub(crate) fn panel_url<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<String> {
    panel(app)?.url().ok().map(|url| url.to_string())
}

pub(crate) fn panel_bounds<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<(f64, f64, f64, f64)> {
    let live = (|| {
        let webview = panel(app)?;
        let bounds = webview.bounds().ok()?;
        let scale = webview.window().scale_factor().ok()?;
        let position = bounds.position.to_logical::<f64>(scale);
        let size = bounds.size.to_logical::<f64>(scale);
        (size.width > 0.0 && size.height > 0.0).then_some((
            position.x,
            position.y,
            size.width,
            size.height,
        ))
    })();
    live.or_else(|| {
        *LAST_PANEL_BOUNDS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    })
}

fn remember_panel_bounds(x: f64, y: f64, width: f64, height: f64) {
    *LAST_PANEL_BOUNDS
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner()) =
        Some((x, y, width.max(1.0), height.max(1.0)));
}

fn panel<R: tauri::Runtime>(app: &AppHandle<R>) -> Option<Webview<R>> {
    app.get_webview(BROWSER_PANEL_LABEL)
}

/// 面板 CDP 端口文件：browser-control 桥接脚本凭它接管面板。
/// 放在 browser-control/ 目录下，与设置、桥接脚本同住。
pub(crate) fn panel_cdp_file(config_dir: &Path) -> PathBuf {
    config_dir.join("browser-control").join("panel-cdp.json")
}

/// 面板独立 user data 目录：与主窗口隔离，才能拥有只暴露面板自己的 CDP 端口
/// （WebView2 按数据目录共享浏览器进程，共享时调试端口会连带暴露主窗口 webview，
/// 它持有 Tauri IPC，绝不能进 CDP 暴露面）。
#[cfg(target_os = "windows")]
fn panel_profile_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("browser-panel-profile")
}

/// 取一个空闲的本机端口（绑定 :0 后立刻释放，局部竞态可接受）。
#[cfg(target_os = "windows")]
fn free_localhost_port() -> u16 {
    std::net::TcpListener::bind(("127.0.0.1", 0))
        .and_then(|listener| listener.local_addr())
        .map(|addr| addr.port())
        .unwrap_or(0)
}

/// 过滤 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 里的调试通道 flag（port 与 pipe
/// 两种形态）：WebView2 会把该环境变量并入每个环境，且同名 flag 以环境变量为
/// 尊——不剥掉的话面板端口会被静默劫持（典型后果：与主环境调试端口相撞，
/// 面板 CDP 绑定失败）。
#[cfg(target_os = "windows")]
fn strip_remote_debugging_flags(raw: &str) -> String {
    raw.split_whitespace()
        .filter(|arg| {
            !arg.starts_with("--remote-debugging-port")
                && !arg.starts_with("--remote-debugging-pipe")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(target_os = "windows")]
fn sanitize_webview2_env_port_override() {
    if let Some(value) = std::env::var_os("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
        let filtered = strip_remote_debugging_flags(&value.to_string_lossy());
        // SAFETY: 进程级环境变量，面板创建前的主窗口环境已建好，不受影响。
        std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", filtered);
    }
}

/// 独立环境的浏览器参数。覆盖 additional_browser_args 会丢弃 wry 默认值
/// （tauri 文档警告），必须自带三件套；再加面板专属 CDP 端口。
#[cfg(target_os = "windows")]
fn panel_browser_args(port: u16) -> String {
    format!(
        "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --remote-debugging-port={port}"
    )
}

#[cfg(target_os = "windows")]
fn write_panel_cdp_file(config_dir: &Path, port: u16) -> Result<(), String> {
    use std::io::Write;
    let path = panel_cdp_file(config_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let started_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    let payload = serde_json::json!({ "port": port, "startedAt": started_at });
    let temp = path.with_extension("tmp");
    let mut file = std::fs::File::create(&temp).map_err(|e| e.to_string())?;
    file.write_all(
        serde_json::to_vec_pretty(&payload)
            .unwrap_or_default()
            .as_slice(),
    )
    .map_err(|e| e.to_string())?;
    drop(file);
    std::fs::rename(&temp, &path).map_err(|e| e.to_string())?;
    Ok(())
}

fn logical_rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        position: Position::Logical(LogicalPosition::new(x, y)),
        size: Size::Logical(LogicalSize::new(width, height)),
    }
}

/// 已存在则按矩形重新定位并显示；否则用初始 URL 在主窗口内创建子 webview。
#[tauri::command]
pub async fn browser_panel_open(
    app: AppHandle,
    panel_id: Option<String>,
    url: String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        if let Some(id) = panel_id {
            open_instance_sync(&app, &id, &url, x, y, width, height)
        } else {
            open_panel_sync(&app, &url, x, y, width, height)
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

fn instance_label(id: &str) -> Result<String, String> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| "无效的浏览器标签 ID".to_string())?;
    Ok(format!("browser-panel-{}", parsed))
}

fn command_panel(app: &AppHandle, id: Option<&str>) -> Result<Webview, String> {
    let label = match id {
        Some(id) => instance_label(id)?,
        None => BROWSER_PANEL_LABEL.to_string(),
    };
    app.get_webview(&label)
        .ok_or_else(|| "浏览器面板尚未创建".to_string())
}

fn open_instance_sync(
    app: &AppHandle,
    id: &str,
    url: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let _lifecycle = panel_lifecycle_lock()?;
    let label = instance_label(id)?;
    if let Some(view) = app.get_webview(&label) {
        view.set_bounds(logical_rect(x, y, width, height))
            .map_err(|e| e.to_string())?;
        return view.show().map_err(|e| e.to_string());
    }
    let window = app.get_window("main").ok_or("未找到主窗口")?;
    let nav_app = app.clone();
    let load_app = app.clone();
    let nav_id = id.to_string();
    let load_id = id.to_string();
    #[allow(unused_mut)]
    let mut builder = WebviewBuilder::new(&label, WebviewUrl::External(validate_panel_url(app, url)?))
        .on_navigation(move |url| {
            if validate_panel_url(&nav_app, url.as_str()).is_err() { return false; }
            let _ = nav_app.emit(
                BROWSER_PANEL_NAVIGATED_EVENT,
                NavigatedPayload {
                    panel_id: Some(nav_id.clone()),
                    url: url.to_string(),
                },
            );
            true
        })
        .on_page_load(move |_, payload| {
            let event = match payload.event() {
                PageLoadEvent::Started => "started",
                PageLoadEvent::Finished => "finished",
            };
            let _ = load_app.emit(
                BROWSER_PANEL_LOAD_EVENT,
                LoadPayload {
                    panel_id: Some(load_id.clone()),
                    event: event.into(),
                    url: payload.url().to_string(),
                },
            );
        });
    // 手动标签共享登录环境，但每个标签的页面和历史独立；不发布 Agent 面板的 CDP 文件。
    #[cfg(target_os = "windows")]
    {
        builder = builder.data_directory(
            crate::providers::config_dir()
                .map_err(|e| e.to_string())?
                .join("browser-manual-tabs"),
        );
    }
    window
        .add_child(
            builder,
            LogicalPosition::new(x, y),
            LogicalSize::new(width, height),
        )
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn browser_panel_close(app: AppHandle, panel_id: Option<String>) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _lifecycle = panel_lifecycle_lock()?;
        let label = match panel_id.as_deref() {
            Some(id) => instance_label(id)?,
            None => BROWSER_PANEL_LABEL.to_string(),
        };
        if let Some(view) = app.get_webview(&label) {
            view.close().map_err(|e| e.to_string())?;
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 面板创建/显示核心（同步）：已存在则 set_bounds+show；否则 add_child 创建。
/// 供 Tauri 命令与 browser_control 的「面板优先自动救场」共用——后者需要在
/// spawn_blocking 线程里带重试地创建面板，不能依赖前端事件链的时序。
pub fn open_panel_sync<R: tauri::Runtime>(
    app: &AppHandle<R>,
    url: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let _lifecycle = panel_lifecycle_lock()?;
    open_panel_locked(app, url, x, y, width, height)
}

/// 调用者已持有生命周期锁；重建不能再进入会取锁的公开 open 入口。
fn open_panel_locked<R: tauri::Runtime>(
    app: &AppHandle<R>,
    url: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    if let Some(webview) = panel(app) {
        webview
            .set_bounds(logical_rect(x, y, width, height))
            .map_err(|e| e.to_string())?;
        webview.show().map_err(|e| e.to_string())?;
        remember_panel_bounds(x, y, width, height);
        return Ok(());
    }
    let initial = validate_panel_url(app, url)?;
    // 恢复失败会留下旧 owned 端点而 registry 已空。每个创建入口都必须
    // 先完成同一退出守卫，不能在下一次 open 时绕过它并覆盖 owner。
    #[cfg(target_os = "windows")]
    retire_owned_panel_endpoint(&OWNED_PANEL_CDP_PORT)?;
    let window = app.get_window("main").ok_or("未找到主窗口")?;
    let nav_app = app.clone();
    let load_app = app.clone();
    #[allow(unused_mut)]
    let mut builder = WebviewBuilder::new(BROWSER_PANEL_LABEL, WebviewUrl::External(initial))
        // 返回 true 放行全部跳转；地址栏由该事件实时回填。
        .on_navigation(move |url| {
            if validate_panel_url(&nav_app, url.as_str()).is_err() { return false; }
            let _ = nav_app.emit(
                BROWSER_PANEL_NAVIGATED_EVENT,
                NavigatedPayload {
                    panel_id: None,
                    url: url.to_string(),
                },
            );
            true
        })
        .on_page_load(move |_, payload| {
            let event = match payload.event() {
                PageLoadEvent::Started => "started",
                PageLoadEvent::Finished => "finished",
            };
            let _ = load_app.emit(
                BROWSER_PANEL_LOAD_EVENT,
                LoadPayload {
                    panel_id: None,
                    event: event.into(),
                    url: payload.url().to_string(),
                },
            );
        });
    // macOS 14+ 的持久面板存储与主应用隔离；不同验收配置目录也不串登录态。
    #[cfg(target_os = "macos")]
    {
        use sha2::{Digest, Sha256};
        let config = crate::providers::config_dir().map_err(|e| e.to_string())?;
        let digest = Sha256::digest(format!("crewvela-browser-panel-v1:{}", config.display()));
        let mut identifier = [0; 16];
        identifier.copy_from_slice(&digest[..16]);
        builder = builder.data_store_identifier(identifier);
    }
    // Windows：独立 WebView2 环境（独立数据目录 + 面板专属 CDP 端口），
    // browser-control 桥接脚本凭端口文件接管本面板；主窗口 webview 不在暴露面。
    #[cfg(target_os = "windows")]
    let (config_dir, port) = {
        let config_dir = crate::providers::config_dir().map_err(|e| e.to_string())?;
        let port = free_localhost_port();
        if port == 0 {
            return Err("无法为浏览器面板分配 CDP 端口".into());
        }
        sanitize_webview2_env_port_override();
        builder = builder
            .data_directory(panel_profile_dir(&config_dir))
            .additional_browser_args(&panel_browser_args(port));
        (config_dir, port)
    };
    let webview = window
        .add_child(
            builder,
            LogicalPosition::new(x, y),
            LogicalSize::new(width, height),
        )
        .map_err(|e| e.to_string())?;
    // 只发布真正创建成功的面板端口，失败创建不能改写正在使用的 CDP 文件。
    #[cfg(target_os = "windows")]
    if let Err(error) = write_panel_cdp_file(&config_dir, port) {
        webview
            .close()
            .map_err(|close_error| format!("{error}；关闭未发布 CDP 的面板失败：{close_error}"))?;
        return Err(error);
    }
    #[cfg(target_os = "windows")]
    OWNED_PANEL_CDP_PORT.store(port, std::sync::atomic::Ordering::Relaxed);
    remember_panel_bounds(x, y, width, height);
    #[cfg(not(target_os = "windows"))]
    let _ = webview;
    Ok(())
}

/// 销毁并重建面板。冷启动竞态窗口内创建的面板 webview 可能丢失
/// additional_browser_args（CDP 端口永不监听）；关闭面板（其独立 WebView2
/// 环境随最后一个 webview 退出，登录态留在磁盘 UDF）后重建即拿到全新环境。
pub fn recreate_panel_sync<R: Runtime>(
    app: &AppHandle<R>,
    url: &str,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let _lifecycle = panel_lifecycle_lock()?;
    if let Some(webview) = panel(app) {
        webview.close().map_err(|e| e.to_string())?;
        // 给 WebView2 环境进程一点退出时间，避免重建时复用旧 browser 进程。
        std::thread::sleep(std::time::Duration::from_millis(350));
    }
    open_panel_locked(app, url, x, y, width, height)
}

#[cfg(target_os = "windows")]
fn retire_owned_panel_endpoint(owned_port: &std::sync::atomic::AtomicU16) -> Result<(), String> {
    let previous_port = owned_port.load(std::sync::atomic::Ordering::Relaxed);
    #[cfg(target_os = "windows")]
    if previous_port != 0 {
        let port = previous_port;
        // MCP 创建的无窗口页面可能使旧环境继续存活。仅关闭本实例原生
        // 创建端点里的残留 page，保留持久 profile 与其它浏览器。
        crate::browser_control::close_panel_cdp_pages(port).map_err(|error| error.to_string())?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while crate::browser_control::panel_port_listening(port)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        if crate::browser_control::panel_port_listening(port) {
            return Err("旧面板环境未退出，无法安全更换 CDP 端口".into());
        }
        owned_port.store(0, std::sync::atomic::Ordering::Relaxed);
        eprintln!("[crewvela] 旧浏览器面板环境已退出：CDP {port}");
    }
    Ok(())
}

#[tauri::command]
pub fn browser_panel_set_bounds(
    app: AppHandle,
    panel_id: Option<String>,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
) -> Result<(), String> {
    let webview = command_panel(&app, panel_id.as_deref())?;
    webview
        .set_bounds(logical_rect(x, y, width, height))
        .map_err(|e| e.to_string())?;
    if panel_id.is_none() {
        remember_panel_bounds(x, y, width, height);
    }
    Ok(())
}

#[tauri::command]
pub fn browser_panel_set_visible(
    app: AppHandle,
    panel_id: Option<String>,
    visible: bool,
) -> Result<(), String> {
    let webview = command_panel(&app, panel_id.as_deref())?;
    if visible {
        webview.show()
    } else {
        webview.hide()
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_panel_navigate(
    app: AppHandle,
    panel_id: Option<String>,
    url: String,
) -> Result<(), String> {
    let webview = command_panel(&app, panel_id.as_deref())?;
    webview
        .navigate(validate_panel_url(&app, &url)?)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_panel_history(
    app: AppHandle,
    panel_id: Option<String>,
    action: String,
) -> Result<(), String> {
    let webview = command_panel(&app, panel_id.as_deref())?;
    webview
        .eval(history_script(&action)?)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn browser_panel_current_url(
    app: AppHandle,
    panel_id: Option<String>,
) -> Result<String, String> {
    let webview = command_panel(&app, panel_id.as_deref())?;
    webview
        .url()
        .map(|url| url.to_string())
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manual_panel_labels_are_unique_and_reject_invalid_ids() {
        let a = instance_label("00000000-0000-4000-8000-000000000001").unwrap();
        let b = instance_label("00000000-0000-4000-8000-000000000002").unwrap();
        assert_ne!(a, b);
        assert_ne!(a, BROWSER_PANEL_LABEL);
        assert!(instance_label("../main").is_err());
        assert!(instance_label("main").is_err());
    }

    #[test]
    fn panel_events_identify_manual_instance_and_preserve_default_protocol() {
        let legacy = serde_json::to_value(NavigatedPayload {
            panel_id: None,
            url: "https://example.com".into(),
        })
        .unwrap();
        assert!(legacy.get("panelId").is_none());
        let manual = serde_json::to_value(NavigatedPayload {
            panel_id: Some("test-id".into()),
            url: "https://example.com".into(),
        })
        .unwrap();
        assert_eq!(manual["panelId"], "test-id");
        let load = serde_json::to_value(LoadPayload {
            panel_id: Some("test-id".into()),
            event: "finished".into(),
            url: "https://example.com".into(),
        })
        .unwrap();
        assert_eq!(load["panelId"], "test-id");
    }

    #[test]
    fn cached_logical_bounds_survive_unavailable_controller() {
        let _lifecycle = panel_lifecycle_lock().unwrap();
        let previous = *LAST_PANEL_BOUNDS.lock().unwrap();
        let app = tauri::test::mock_app();
        // 没有可读的原生控制器时，仍恢复 DOM host 的 y120 而不是默认 y64。
        remember_panel_bounds(1024.0, 120.0, 640.0, 760.0);
        let initial = panel_bounds(app.handle());
        remember_panel_bounds(1080.0, 120.0, 580.0, 760.0);
        let resized = panel_bounds(app.handle());
        *LAST_PANEL_BOUNDS.lock().unwrap() = previous;
        assert_eq!(initial, Some((1024.0, 120.0, 640.0, 760.0)));
        assert_eq!(resized, Some((1080.0, 120.0, 580.0, 760.0)));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn failed_endpoint_retirement_keeps_owner_and_retries_before_new_creation() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let owner = std::sync::atomic::AtomicU16::new(port);
        let (release, released) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let mut requests = 0;
            while requests < 2 {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buffer = [0u8; 512];
                let count = stream.read(&mut buffer).unwrap();
                if count == 0 {
                    continue;
                } // TCP 探活。
                assert!(String::from_utf8_lossy(&buffer[..count]).starts_with("GET /json/list "));
                stream
                    .write_all(
                        b"HTTP/1.1 500 Error\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                    )
                    .unwrap();
                requests += 1;
            }
            released.recv().unwrap();
        });
        assert!(retire_owned_panel_endpoint(&owner).is_err());
        assert_eq!(owner.load(std::sync::atomic::Ordering::Relaxed), port);
        assert!(retire_owned_panel_endpoint(&owner).is_err());
        assert_eq!(owner.load(std::sync::atomic::Ordering::Relaxed), port);
        release.send(()).unwrap();
        server.join().unwrap();
        // 环境实际退出后，下一次重试才允许清掉 owner 并继续创建。
        assert!(retire_owned_panel_endpoint(&owner).is_ok());
        assert_eq!(owner.load(std::sync::atomic::Ordering::Relaxed), 0);
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn owned_panel_port_is_not_replaced_by_foreign_port_file() {
        let _lifecycle = panel_lifecycle_lock().unwrap();
        let previous = OWNED_PANEL_CDP_PORT.swap(52341, std::sync::atomic::Ordering::Relaxed);
        let directory =
            std::env::temp_dir().join(format!("cc-owned-panel-{}", uuid::Uuid::new_v4()));
        write_panel_cdp_file(&directory, 52342).unwrap();
        let observed = OWNED_PANEL_CDP_PORT.load(std::sync::atomic::Ordering::Relaxed);
        OWNED_PANEL_CDP_PORT.store(previous, std::sync::atomic::Ordering::Relaxed);
        std::fs::remove_dir_all(directory).unwrap();
        assert_eq!(observed, 52341);
    }

    #[test]
    fn lifecycle_serializes_concurrent_check_and_create() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
        use std::sync::{Arc, Barrier};

        let start = Arc::new(Barrier::new(8));
        let exists = Arc::new(AtomicBool::new(false));
        let creations = Arc::new(AtomicUsize::new(0));
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let start = start.clone();
                let exists = exists.clone();
                let creations = creations.clone();
                scope.spawn(move || {
                    start.wait();
                    let _lifecycle = panel_lifecycle_lock().unwrap();
                    if !exists.load(Ordering::SeqCst) {
                        // 放大 check/create 的窗口，未加锁会创建多个同名面板。
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        creations.fetch_add(1, Ordering::SeqCst);
                        exists.store(true, Ordering::SeqCst);
                    }
                });
            }
        });
        assert!(exists.load(Ordering::SeqCst));
        assert_eq!(creations.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn lifecycle_failure_releases_lock_for_retry() {
        let fail = || -> Result<(), String> {
            let _lifecycle = panel_lifecycle_lock()?;
            Err("模拟创建失败".into())
        };
        assert_eq!(fail().unwrap_err(), "模拟创建失败");
        assert!(panel_lifecycle_lock().is_ok());
    }

    #[test]
    fn validate_url_accepts_only_web_pages() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://localhost:3000/docs").is_ok());
        assert!(validate_url("javascript:alert(1)").is_err());
        assert!(validate_url("http://tauri.localhost/index.html").is_err());
        assert!(validate_url("https://asset.localhost/private").is_err());
        assert!(validate_url("tauri://localhost/index.html").is_err());
        assert!(validate_url("file:///C:/private.txt").is_err());
        assert!(validate_url("data:text/html,hello").is_err());
        assert!(validate_url("ftp://example.com").is_err());
        assert!(validate_url("example.com").is_err());
        assert!(validate_url("").is_err());
    }

    #[test]
    fn history_script_maps_known_actions() {
        assert_eq!(history_script("back").unwrap(), "history.back()");
        assert_eq!(history_script("forward").unwrap(), "history.forward()");
        assert_eq!(history_script("reload").unwrap(), "location.reload()");
        assert!(history_script("home").is_err());
    }

    #[test]
    fn panel_cdp_file_lives_under_browser_control_dir() {
        #[cfg(windows)]
        let path = panel_cdp_file(Path::new(r"C:\conf"));
        #[cfg(windows)]
        assert_eq!(
            path,
            PathBuf::from(r"C:\conf\browser-control\panel-cdp.json")
        );
        #[cfg(not(windows))]
        assert_eq!(
            panel_cdp_file(Path::new("/fixture/conf")),
            PathBuf::from("/fixture/conf/browser-control/panel-cdp.json")
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn panel_browser_args_keep_wry_defaults_and_cdp_port() {
        let args = panel_browser_args(52341);
        assert!(args.contains("--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection"));
        assert!(args.contains("--remote-debugging-port=52341"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn write_panel_cdp_file_roundtrips_port() {
        let dir = std::env::temp_dir().join(format!("cc-panel-cdp-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        write_panel_cdp_file(&dir, 52342).unwrap();
        let raw = std::fs::read_to_string(panel_cdp_file(&dir)).unwrap();
        let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(value["port"], 52342);
        assert!(value["startedAt"].is_u64());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn strip_remote_debugging_flags_only_removes_port_flags() {
        assert_eq!(
            strip_remote_debugging_flags(
                "--remote-debugging-port=9224 --proxy-server=http://x --remote-debugging-pipe"
            ),
            "--proxy-server=http://x"
        );
        assert_eq!(strip_remote_debugging_flags(""), "");
        assert_eq!(
            strip_remote_debugging_flags("--autoplay-policy=no-user-gesture-required"),
            "--autoplay-policy=no-user-gesture-required"
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn free_localhost_port_returns_usable_port() {
        let port = free_localhost_port();
        assert!(port > 0);
        // 立刻再绑一次验证端口确实可用（非严格保证，但覆盖典型竞态）。
        assert!(std::net::TcpListener::bind(("127.0.0.1", port)).is_ok());
    }
}
