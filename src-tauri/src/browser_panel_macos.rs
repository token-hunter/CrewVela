//! macOS panel automation. This service never exposes the application's WebView.
//! WKWebView has no Chromium CDP endpoint; Node MCP talks to this authenticated,
//! loopback-only adapter instead. All WebKit work is dispatched by Tauri.
use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;
use tauri::{Emitter, Manager};

const HEADER_LIMIT: usize = 8192;
const BODY_LIMIT: usize = 256 * 1024;
const VALUE_LIMIT: usize = 2 * 1024 * 1024;
const PNG_LIMIT: usize = 8 * 1024 * 1024;
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(12);

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    action: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    script: Option<String>,
}

/// Parse a single request. No CORS, chunked encoding, request pipelining, or
/// browser-origin requests are accepted. Authentication happens before the body
/// is interpreted, and both headers and body have independent size limits.
fn read_command(reader: &mut impl Read, token: &str) -> Result<Command> {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= HEADER_LIMIT { bail!("请求头过大"); }
        let mut byte = [0];
        reader.read_exact(&mut byte).context("读取请求头失败")?;
        header.push(byte[0]);
    }
    let header = std::str::from_utf8(&header).context("请求头编码无效")?;
    let mut lines = header.split("\r\n");
    if lines.next() != Some("POST /command HTTP/1.1") { bail!("请求路径无效"); }
    let mut authorized = false;
    let mut length = None;
    let mut host = false;
    let mut content_type = false;
    for line in lines.filter(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':').ok_or_else(|| anyhow!("请求头无效"))?;
        let value = value.trim();
        match name.to_ascii_lowercase().as_str() {
            "authorization" => {
                if authorized || value != format!("Bearer {token}") { bail!("认证失败"); }
                authorized = true;
            }
            "content-length" => {
                if length.is_some() { bail!("重复的请求长度"); }
                length = Some(value.parse::<usize>().context("请求长度无效")?);
            }
            "host" => {
                if host || !value.strip_prefix("127.0.0.1:").is_some_and(|port| port.parse::<u16>().is_ok_and(|p| p != 0)) {
                    bail!("仅接受本机请求");
                }
                host = true;
            }
            "content-type" => {
                if content_type || value != "application/json" { bail!("请求类型无效"); }
                content_type = true;
            }
            "origin" | "referer" | "transfer-encoding" => bail!("不接受网页来源或分块请求"),
            _ => {}
        }
    }
    if !authorized || !host || !content_type { bail!("请求未认证或缺少必要请求头"); }
    let length = length.filter(|length| (1..=BODY_LIMIT).contains(length))
        .ok_or_else(|| anyhow!("请求长度超限或缺失"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).context("读取请求内容失败")?;
    serde_json::from_slice(&body).context("命令格式无效")
}

fn check_enabled(config_dir: &Path) -> Result<()> {
    if crate::test_profile::active() { bail!("隔离测试模式禁用浏览器接管"); }
    let settings: crate::browser_control::BrowserControlSettings = serde_json::from_slice(
        &std::fs::read(config_dir.join("browser-control.json")).context("浏览器控制配置不可用")?
    ).context("浏览器控制配置无效")?;
    if !settings.enabled || !settings.prefer_panel { bail!("内置浏览器面板控制未开启"); }
    Ok(())
}

fn ensure_panel(app: &tauri::AppHandle, initial_url: &str) -> Result<tauri::Webview> {
    if let Some(view) = app.get_webview(crate::browser_panel::BROWSER_PANEL_LABEL) {
        // The frontend owns visibility, including the modal overlay guard.
        // Native show() here would lift the child WebView over a settings modal.
        app.emit(crate::browser_control::PANEL_NEEDED_EVENT, json!({ "url": view.url()?.to_string() }))?;
        return Ok(view);
    }
    let window = app.get_window("main").context("未找到主窗口")?;
    let scale = window.scale_factor()?;
    let size = window.inner_size()?;
    let width = size.width as f64 / scale;
    let height = size.height as f64 / scale;
    let panel_width = (width * 0.4).clamp(380.0, 720.0);
    crate::browser_panel::open_panel_sync(app, initial_url,
        width - panel_width, 64.0, panel_width, (height - 110.0).max(360.0))
        .map_err(anyhow::Error::msg)?;
    let view = app.get_webview(crate::browser_panel::BROWSER_PANEL_LABEL).context("浏览器面板创建失败")?;
    view.hide()?;
    // Emit only after creation and hide have completed: the frontend's event
    // handler can now size/show the existing view if no modal is open.
    app.emit(crate::browser_control::PANEL_NEEDED_EVENT, json!({ "url": view.url()?.to_string() }))?;
    Ok(view)
}

fn panel_state(app: &tauri::AppHandle) -> Result<Value> {
    let Some(view) = app.get_webview(crate::browser_panel::BROWSER_PANEL_LABEL) else {
        return Ok(json!({ "url": null, "loading": false }));
    };
    let (sender, receiver) = mpsc::sync_channel(1);
    view.with_webview(move |platform| {
        // Read WebKit's own loading state on its owning main thread. In
        // particular, an old document reporting readyState=complete does not
        // mean a newly requested navigation has completed.
        let value = unsafe {
            let view: &objc2_web_kit::WKWebView = &*platform.inner().cast();
            let url = view.URL().and_then(|url| url.absoluteString()).map(|url| url.to_string());
            json!({ "url": url, "loading": view.isLoading() })
        };
        let _ = sender.try_send(value);
    })?;
    receiver.recv_timeout(CALLBACK_TIMEOUT).context("等待浏览器面板状态超时")
}

fn evaluate(view: &tauri::Webview, script: &str) -> Result<Value> {
    let (sender, receiver) = mpsc::sync_channel(1);
    // Exceptions are explicitly serialized; otherwise WebKit evaluation failure
    // may be indistinguishable from the JavaScript value null.
    // Always hand Wry a primitive string. Its native serializer assumes a
    // JSON-compatible result; arbitrary browser_evaluate values (DOM nodes,
    // cycles, BigInt) must not reach that serializer and panic the UI thread.
    let wrapped = format!("(()=>{{try{{return ''+JSON.stringify({{ok:true,value:({script})}})}}catch(e){{return ''+JSON.stringify({{ok:false,error:''+e}})}}}})()");
    view.eval_with_callback(wrapped, move |raw| { let _ = sender.try_send(raw); })?;
    let raw = receiver.recv_timeout(CALLBACK_TIMEOUT).context("等待网页脚本结果超时")?;
    if raw.len() > VALUE_LIMIT { bail!("页面结果超过大小限制，请缩小查询范围"); }
    let serialized: String = serde_json::from_str(&raw).context("网页脚本未返回可序列化结果")?;
    let result: Value = serde_json::from_str(&serialized).context("网页脚本未返回可序列化结果")?;
    if result["ok"] != true { bail!("{}", result["error"].as_str().unwrap_or("网页脚本执行失败")); }
    Ok(result["value"].clone())
}

fn screenshot(view: &tauri::Webview) -> Result<Value> {
    use block2::RcBlock;
    use objc2::AnyThread;
    use objc2_app_kit::{NSBitmapImageFileType, NSBitmapImageRep, NSImage};
    use objc2_foundation::{NSDictionary, NSError};
    use objc2_web_kit::WKWebView;
    let (sender, receiver) = mpsc::sync_channel(1);
    view.with_webview(move |platform| {
        // Tauri invokes this closure on the main thread, where the borrowed
        // WKWebView and WebKit's completion callback are valid. No NS objects
        // escape to the HTTP worker; only owned PNG bytes cross the channel.
        unsafe {
            let webview: &WKWebView = &*platform.inner().cast();
            let callback = RcBlock::new(move |image: *mut NSImage, error: *mut NSError| {
                let result = (|| -> Result<Vec<u8>> {
                    if let Some(error) = error.as_ref() { bail!("截图失败：{}", error.localizedDescription()); }
                    let image = image.as_ref().context("WebKit 未返回截图")?;
                    let tiff = image.TIFFRepresentation().context("截图图像无法编码")?;
                    let bitmap = NSBitmapImageRep::initWithData(NSBitmapImageRep::alloc(), &tiff)
                        .context("截图位图无法解码")?;
                    let png = bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
                        .context("截图 PNG 编码失败")?;
                    if png.len() > PNG_LIMIT { bail!("截图超过大小限制"); }
                    Ok(png.to_vec())
                })().map_err(|error| format!("{error:#}"));
                let _ = sender.try_send(result);
            });
            webview.takeSnapshotWithConfiguration_completionHandler(None, &callback);
        }
    })?;
    let png = receiver.recv_timeout(CALLBACK_TIMEOUT).context("等待网页截图超时")?
        .map_err(anyhow::Error::msg)?;
    Ok(json!(base64::engine::general_purpose::STANDARD.encode(png)))
}

fn execute(app: &tauri::AppHandle, config_dir: &Path, command: Command) -> Result<Value> {
    check_enabled(config_dir)?;
    if command.action == "state" {
        // Pure observation: no activation event, creation or visibility change.
        return panel_state(app);
    }
    if command.action == "close" {
        // Closing or inspecting an unopened panel must not create it.
        if let Some(view) = app.get_webview(crate::browser_panel::BROWSER_PANEL_LABEL) { view.close()?; }
        app.emit("browser-panel-agent-closed", ())?;
        return Ok(Value::Null);
    }
    if !matches!(command.action.as_str(), "ensure" | "navigate" | "eval" | "screenshot" | "back") {
        bail!("不支持的浏览器面板命令");
    }
    let initial_url = if command.action == "navigate" {
        let url = command.url.as_deref().context("缺少 URL")?;
        crate::browser_panel::validate_url(url).map_err(anyhow::Error::msg)?;
        url
    } else { "https://www.bing.com" };
    let view = ensure_panel(app, initial_url)?;
    match command.action.as_str() {
        "ensure" => Ok(json!({ "url": view.url()?.to_string() })),
        "navigate" => {
            crate::browser_panel::browser_panel_navigate(app.clone(), None, initial_url.into())
                .map_err(anyhow::Error::msg)?;
            Ok(Value::Null)
        }
        "eval" => evaluate(&view, command.script.as_deref().context("缺少脚本")?),
        "screenshot" => screenshot(&view),
        "back" => {
            crate::browser_panel::browser_panel_history(app.clone(), None, "back".into())
                .map_err(anyhow::Error::msg)?;
            Ok(Value::Null)
        }
        _ => unreachable!(),
    }
}

fn serve(mut stream: TcpStream, app: &tauri::AppHandle, config_dir: &Path, token: &str) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let (status, response) = match read_command(&mut stream, token) {
        Ok(command) => ("200 OK", match execute(app, config_dir, command) {
            Ok(value) => json!({ "value": value }),
            Err(error) => json!({ "error": format!("{error:#}") }),
        }),
        // Do not reflect a credential or malformed request body into the reply.
        Err(_) => ("403 Forbidden", json!({ "error": "invalid or unauthorized request" })),
    };
    let body = response.to_string();
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\n\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes());
}

pub fn start_service(app: &tauri::AppHandle, config_dir: &Path) -> Result<()> {
    if crate::test_profile::active() { bail!("隔离测试模式禁用浏览器接管服务"); }
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let token = uuid::Uuid::new_v4().to_string();
    let directory = config_dir.join("browser-control");
    std::fs::create_dir_all(&directory)?;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700))?;
    let temp = directory.join(format!("panel-macos-{}.tmp", uuid::Uuid::new_v4()));
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temp)?;
    file.write_all(&serde_json::to_vec(&json!({ "port": port, "token": token }))?)?;
    file.sync_all()?;
    std::fs::rename(&temp, directory.join("panel-macos.json"))?;
    let app = app.clone();
    let config_dir = config_dir.to_path_buf();
    std::thread::Builder::new().name("macos-browser-panel".into()).spawn(move || {
        for incoming in listener.incoming() {
            let Ok(stream) = incoming else { break };
            // A single worker serializes page actions and callback results.
            serve(stream, &app, &config_dir, &token);
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(headers: &str, body: &str) -> Vec<u8> {
        format!("POST /command HTTP/1.1\r\nHost: 127.0.0.1:1234\r\nAuthorization: Bearer test-token\r\nContent-Type: application/json\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len()).into_bytes()
    }
    #[test]
    fn authenticated_bounded_request_is_parsed() {
        let bytes = request("", r#"{"action":"eval","script":"document.title"}"#);
        let command = read_command(&mut bytes.as_slice(), "test-token").unwrap();
        assert_eq!(command.action, "eval");
        assert_eq!(command.script.as_deref(), Some("document.title"));
    }
    #[test]
    fn rejects_browser_origins_credentials_ambiguous_framing_and_extra_fields() {
        for header in ["Origin: null\r\n", "Origin: https://example.com\r\n", "Referer: https://example.com\r\n", "Transfer-Encoding: chunked\r\n", "Content-Length: 1\r\n", "Authorization: Bearer test-token\r\n"] {
            assert!(read_command(&mut request(header, r#"{"action":"ensure"}"#).as_slice(), "test-token").is_err(), "{header}");
        }
        assert!(read_command(&mut request("", r#"{"action":"ensure"}"#).as_slice(), "wrong-token").is_err());
        assert!(read_command(&mut request("", r#"{"action":"eval","webview":"main"}"#).as_slice(), "test-token").is_err());
    }
    #[test]
    fn rejects_oversized_headers_and_bodies_and_unexpected_path() {
        assert!(read_command(&mut vec![b'x'; HEADER_LIMIT + 1].as_slice(), "test-token").is_err());
        let large = "x".repeat(BODY_LIMIT + 1);
        assert!(read_command(&mut request("", &large).as_slice(), "test-token").is_err());
        let wrong = String::from_utf8(request("", r#"{"action":"ensure"}"#)).unwrap().replace("/command", "/eval");
        assert!(read_command(&mut wrong.as_bytes(), "test-token").is_err());
    }

    #[test]
    fn disabled_missing_or_invalid_settings_fail_closed() {
        let directory = std::env::temp_dir().join(format!("crewvela-macos-panel-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        assert!(check_enabled(&directory).is_err());
        for settings in [
            json!({ "enabled": false, "preferPanel": true }),
            json!({ "enabled": true, "preferPanel": false }),
            json!({ "enabled": false, "preferPanel": false }),
        ] {
            std::fs::write(directory.join("browser-control.json"), settings.to_string()).unwrap();
            assert!(check_enabled(&directory).is_err());
        }
        std::fs::write(directory.join("browser-control.json"), "not JSON").unwrap();
        assert!(check_enabled(&directory).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
