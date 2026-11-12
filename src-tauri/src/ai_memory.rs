//! ai-memory 共享记忆服务管理：检测 / 安装 / 启停 / 项目交接棒查询。
//! 服务是独立基础设施进程：CrewVela 只负责安装与托管启动，不随本应用
//! 退出而停止。普通停止仅使用本次持有的句柄；Windows 显式更新可在核验
//! 监听进程、用户、程序与完整启动参数后取得旧实例服务的句柄。
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;

use crate::sessions::SessionManager;

#[path = "ai_memory_verify.rs"]
mod verification;
#[path = "ai_memory_zcode.rs"]
pub mod zcode;
#[path = "ai_memory_installation.rs"]
mod installation;
#[path = "ai_memory_release.rs"]
mod releases;
#[path = "ai_memory_auth.rs"]
mod auth;
#[path = "ai_memory_update.rs"]
mod updates;
#[cfg(windows)]
#[path = "ai_memory_process.rs"]
mod service_process;
#[tauri::command]
pub async fn ai_memory_recover_update(confirmed: bool) -> Result<(), String> { updates::recover_update(confirmed).await }
#[path = "ai_memory_onboarding.rs"]
pub mod onboarding;
#[tauri::command]
pub async fn ai_memory_check(agent: SetupAgent, project_dir: String) -> Result<verification::CheckReport, String> {
    verification::ai_memory_check(agent, project_dir).await
}

const DEFAULT_SERVER_URL: &str = "http://127.0.0.1:49374";
static SERVICE_LIFECYCLE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Codex reads its own MCP registration. Only recover the installed local service
/// it is configured to use; custom transports and remote services remain external.
pub(crate) fn ensure_codex_service() -> Result<()> {
    if server_url_override().is_some() { return Ok(()); }
    let (config, _) = verification::config_paths(SetupAgent::Codex, Path::new("."))?;
    ensure_codex_service_with(&config, &resolved_server_url()?, || {
        // Native ACP startup is synchronous and can run on a Tokio worker. A
        // dedicated thread avoids entering another runtime on that worker.
        std::thread::spawn(|| {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(serve())?;
            Ok::<_, anyhow::Error>(())
        })
        .join()
        .map_err(|_| anyhow::anyhow!("ai-memory 启动线程异常退出"))?
    })
}

fn ensure_codex_service_with(config: &Path, base: &str, start: impl FnOnce() -> Result<()>) -> Result<()> {
    // Environment overrides are external endpoints. Only recover our configured listener.
    let Ok(base_url) = reqwest::Url::parse(base) else { return Ok(()); };
    if base_url.scheme() != "http" || !matches!(base_url.host_str(), Some("127.0.0.1" | "[::1]" | "::1")) { return Ok(()); }
    let Some(config) = verification::read_config(config)? else { return Ok(()); };
    let Some(entry) = config.pointer("/mcp_servers/ai-memory") else { return Ok(()); };
    if entry["enabled"] == Value::Bool(false) || entry["disabled"] == Value::Bool(true) {
        return Ok(());
    }
    if entry["url"].as_str().is_some_and(|url| verification::endpoint_matches(url, base)) {
        start().context("Codex Chat 的本机 ai-memory 服务启动失败")?;
    }
    Ok(())
}

async fn service_lifecycle_lock(timeout: Duration) -> Result<tokio::sync::MutexGuard<'static, ()>> {
    tokio::time::timeout(timeout, SERVICE_LIFECYCLE.lock())
        .await
        .context("等待 ai-memory 服务启停操作超时，请稍后重试")
}

async fn wait_for_service_startup<Running, Health, HealthFuture, Cleanup>(
    budget: Duration,
    mut is_running: Running,
    mut health: Health,
    mut cleanup: Cleanup,
) -> Result<()>
where
    Running: FnMut() -> Result<bool>,
    Health: FnMut() -> HealthFuture,
    HealthFuture: std::future::Future<Output = std::result::Result<Value, ServiceHealthError>>,
    Cleanup: FnMut() -> Result<()>,
{
    let deadline = tokio::time::Instant::now() + budget;
    let mut last_error = None;
    let failure = loop {
        if tokio::time::Instant::now() >= deadline {
            break anyhow::anyhow!("ai-memory 服务就绪检查超出总时限：{}", last_error.as_deref().unwrap_or("尚未返回健康状态"));
        }
        match is_running() {
            Ok(true) => {},
            Ok(false) => break anyhow::anyhow!("ai-memory 服务进程在就绪前退出"),
            Err(error) => break error.context("检查本次启动的 ai-memory 进程失败"),
        }
        // Each request and the interval consume the same absolute budget.
        // A 2s HTTP timeout must not multiply a nominal 15s startup into 75s.
        match tokio::time::timeout_at(deadline, health()).await {
            Ok(Ok(_)) => return Ok(()),
            Ok(Err(error)) => last_error = Some(error.to_string()),
            Err(_) => break anyhow::anyhow!("ai-memory 服务健康请求超出启动总时限：{}", last_error.as_deref().unwrap_or("请求未完成")),
        }
        tokio::time::sleep_until(deadline.min(tokio::time::Instant::now() + Duration::from_millis(500))).await;
    };
    cleanup().with_context(|| format!("{failure:#}；清理本次启动的 ai-memory 进程失败"))?;
    Err(failure)
}

fn run_memory_init(command: Command, timeout: Duration) -> Result<()> {
    let result = run_setup_command(command, timeout).context("执行 ai-memory init 失败")?;
    if !result.success {
        bail!("ai-memory init {}（退出码 {:?}）：{} {}",
            if result.timed_out { "超时，已终止" } else { "失败" },
            result.exit_code, result.stderr.trim(), result.stdout.trim());
    }
    Ok(())
}

fn server_url_override() -> Option<String> {
    std::env::var("AI_MEMORY_SERVER_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim_end_matches('/').to_owned())
}

fn managed_server_url() -> Result<String> {
    let path = data_dir()?.join("config.toml");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(DEFAULT_SERVER_URL.into()),
        Err(error) => return Err(error).context("无法读取记忆服务监听配置"),
    };
    managed_url_from_config(&text)
}

fn managed_url_from_config(text: &str) -> Result<String> {
    let config: toml::Value = toml::from_str(text).context("记忆服务 config.toml 格式错误")?;
    let Some(bind) = config.get("bind") else { return Ok(DEFAULT_SERVER_URL.into()); };
    let bind: std::net::SocketAddr = bind.as_str().context("记忆服务 bind 必须是监听地址字符串")?
        .parse().context("记忆服务 bind 不是有效的 IP 和端口")?;
    if !bind.ip().is_loopback() || bind.port() == 0 { bail!("托管记忆服务必须使用固定的本机回环监听地址"); }
    Ok(format!("http://{bind}"))
}

fn resolved_server_url() -> Result<String> {
    server_url_override().map(Ok).unwrap_or_else(managed_server_url)
}

fn managed_dir() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("ai-memory"))
}

fn data_dir() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("ai-memory-data"))
}

/// Child 持有 OS 进程句柄；磁盘上的旧 PID 不能证明进程归属，禁止据此终止进程。
#[derive(Default)]
struct ManagedService {
    child: Option<Child>,
}

impl ManagedService {
    fn is_running(&mut self) -> Result<bool> {
        let Some(child) = self.child.as_mut() else { return Ok(false); };
        if child.try_wait().context("检查托管 ai-memory 进程失败")?.is_some() {
            self.child = None;
            return Ok(false);
        }
        Ok(true)
    }

    fn stop(&mut self) -> Result<()> {
        if !self.is_running()? {
            bail!("本次应用未持有正在运行的 ai-memory 服务句柄；外部服务或应用重启前的服务请手动停止");
        }
        let child = self.child.as_mut().expect("live owned child");
        child.kill().context("停止托管 ai-memory 失败")?;
        child.wait().context("等待托管 ai-memory 退出失败")?;
        self.child = None;
        Ok(())
    }
}

static MANAGED_SERVICE: std::sync::Mutex<ManagedService> = std::sync::Mutex::new(ManagedService { child: None });

fn managed_service() -> Result<std::sync::MutexGuard<'static, ManagedService>> {
    MANAGED_SERVICE.lock().map_err(|_| anyhow::anyhow!("ai-memory 托管进程状态锁异常"))
}

/// 可执行文件定位：环境变量覆盖 → CrewVela 托管目录 → PATH。
fn locate_executable() -> Option<(PathBuf, &'static str)> {
    if let Some(path) = std::env::var_os("CREWVELA_AI_MEMORY_EXECUTABLE") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Some((path, "custom"));
        }
    }
    let exe_name = if cfg!(windows) {
        "ai-memory.exe"
    } else {
        "ai-memory"
    };
    let managed = managed_dir().ok()?.join(exe_name);
    if managed.is_file() {
        return Some((managed, "managed"));
    }
    let discovered = which(exe_name)?;
    Some((discovered, "path"))
}

fn which(name: &str) -> Option<PathBuf> {
    which::which(name).ok()
}

#[derive(Debug)]
struct ServiceHealthError { state: &'static str, message: String }

impl std::fmt::Display for ServiceHealthError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { formatter.write_str(&self.message) }
}
impl std::error::Error for ServiceHealthError {}

fn connection_was_refused(error: &(dyn std::error::Error + 'static)) -> bool {
    let mut cause: Option<&(dyn std::error::Error + 'static)> = Some(error);
    let mut refused = false;
    while let Some(current) = cause {
        if current.downcast_ref::<std::io::Error>().is_some_and(|error| error.kind() == std::io::ErrorKind::ConnectionRefused) { refused = true; }
        cause = current.source();
    }
    refused
}

fn service_transport_error(error: reqwest::Error) -> ServiceHealthError {
    let refused = connection_was_refused(&error);
    let (state, message) = if error.is_timeout() { ("timeout", "记忆服务响应超时") }
        else if refused { ("offline", "记忆服务拒绝连接，当前监听地址没有可用服务") }
        else { ("connection_error", "无法连接记忆服务，请检查地址和本机网络权限") };
    ServiceHealthError { state, message: message.into() }
}

fn authenticated_request(client: &reqwest::Client, url: String) -> Result<reqwest::RequestBuilder> {
    let request = client.get(&url);
    Ok(match auth::token(&data_dir()?, &url)? { Some(token) => request.bearer_auth(token), None => request })
}

fn loopback_socket_address(base: &str) -> Option<std::net::SocketAddr> {
    let url = reqwest::Url::parse(base).ok()?;
    if url.scheme() != "http" || !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let ip = url.host_str()?.trim_matches(['[', ']']).parse::<std::net::IpAddr>().ok()?;
    let port = url.port_or_known_default()?;
    (ip.is_loopback() && port != 0).then(|| std::net::SocketAddr::new(ip, port))
}

fn configure_service_client(base: &str, builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    // The local infrastructure endpoint must never be sent through a user's
    // outbound HTTP proxy. Remote/custom endpoints retain their proxy policy.
    if loopback_socket_address(base).is_some() { builder.no_proxy() } else { builder }
}

fn service_client(base: &str) -> Result<reqwest::Client> {
    configure_service_client(base, reqwest::Client::builder()).build()
        .context("创建记忆服务 HTTP 客户端失败")
}

fn managed_listener_is_unbound(base: &str) -> Result<bool> {
    let Some(address) = loopback_socket_address(base) else { return Ok(false); };
    let socket = socket2::Socket::new(socket2::Domain::for_address(address), socket2::Type::STREAM, Some(socket2::Protocol::TCP))
        .context("创建记忆服务监听探测 socket 失败")?;
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Networking::WinSock::{setsockopt, WSAGetLastError, SOCKET_ERROR, SOL_SOCKET, SO_EXCLUSIVEADDRUSE};
        // Windows allows SO_REUSEADDR listeners to share a port. An exclusive
        // unbound probe cannot steal or temporarily shadow another listener.
        let enabled: i32 = 1;
        let outcome = unsafe { setsockopt(socket.as_raw_socket() as _, SOL_SOCKET, SO_EXCLUSIVEADDRUSE,
            (&enabled as *const i32).cast(), std::mem::size_of::<i32>() as i32) };
        if outcome == SOCKET_ERROR {
            return Err(std::io::Error::from_raw_os_error(unsafe { WSAGetLastError() }))
                .context("启用记忆服务端口独占探测失败");
        }
    }
    match socket.bind(&address.into()) {
        Ok(()) => Ok(true),
        Err(error) if matches!(error.kind(), std::io::ErrorKind::AddrInUse | std::io::ErrorKind::PermissionDenied) => Ok(false),
        Err(error) => Err(error).context("检查记忆服务监听地址失败"),
    }
}

async fn service_json(request: reqwest::RequestBuilder) -> std::result::Result<Value, ServiceHealthError> {
    let response = request.send().await.map_err(service_transport_error)?;
    service_response_json(response).await
}

async fn service_response_json(response: reqwest::Response) -> std::result::Result<Value, ServiceHealthError> {
    if !response.status().is_success() {
        let status = response.status();
        return Err(ServiceHealthError {
            state: if matches!(status.as_u16(), 401 | 403) { "authentication_error" } else { "http_error" },
            message: if matches!(status.as_u16(), 401 | 403) { format!("记忆服务认证失败（HTTP {status}），请核对访问令牌") }
                else { format!("记忆服务接口返回 HTTP {status}") },
        });
    }
    response.json().await.map_err(|error| if error.is_timeout() {
        service_transport_error(error)
    } else {
        ServiceHealthError { state: "invalid_response", message: "记忆服务返回无效 JSON".into() }
    })
}

async fn admin_status_at(client: &reqwest::Client, base: &str) -> std::result::Result<Value, ServiceHealthError> {
    let request = authenticated_request(client, format!("{}/admin/status", base.trim_end_matches('/'))).map_err(|_| ServiceHealthError { state:"authentication_error", message:"无法读取本地记忆认证配置，请核对访问令牌文件".into() })?;
    let value = service_json(request.timeout(Duration::from_secs(2))).await?;
    if value.get("version").and_then(Value::as_str).is_none_or(str::is_empty) || !value.get("counts").is_some_and(Value::is_object) {
        return Err(ServiceHealthError { state: "invalid_response", message: "记忆服务状态响应缺少 version 或 counts，无法确认服务可用".into() });
    }
    Ok(value)
}

async fn managed_status_for_data(client: &reqwest::Client, base: &str, data: &Path) -> std::result::Result<Value, ServiceHealthError> {
    if managed_listener_is_unbound(base).map_err(|_| ServiceHealthError {state:"configuration_error",message:"无法检查本地记忆监听地址".into()})? {
        return Err(ServiceHealthError {state:"offline",message:"本地记忆服务未启动".into()});
    }
    let headers = auth::headers(data, base).map_err(|_| ServiceHealthError {state:"authentication_error",message:"本地记忆认证配置无法读取".into()})?;
    let mut request = client.get(format!("{}/admin/status", base.trim_end_matches('/'))).timeout(Duration::from_secs(2));
    for (key,value) in headers { request = request.header(key,value); }
    let value = service_json(request).await?;
    if value.get("version").and_then(Value::as_str).is_none_or(str::is_empty) || !value.get("counts").is_some_and(Value::is_object) {
        return Err(ServiceHealthError {state:"invalid_response",message:"记忆服务状态响应缺少 version 或 counts".into()});
    }
    Ok(value)
}

async fn managed_admin_status_at(client: &reqwest::Client, base: &str) -> std::result::Result<Value, ServiceHealthError> {
    // Some Windows firewall policies drop connections to an unbound local
    // port instead of returning ConnectionRefused. Prove listener absence via
    // exclusive bind, so a stopped managed service is still recoverable.
    // Environment overrides remain external and are never auto-recovered.
    match managed_listener_is_unbound(base) {
        Ok(true) => return Err(ServiceHealthError { state: "offline", message: "记忆服务未监听当前托管地址".into() }),
        Ok(false) => {},
        Err(error) => return Err(ServiceHealthError { state: "connection_error", message: format!("{error:#}") }),
    }
    admin_status_at(client, base).await
}

async fn admin_status(client: &reqwest::Client) -> std::result::Result<Value, ServiceHealthError> {
    match server_url_override() {
        Some(base) => admin_status_at(client, &base).await,
        None => {
            let base = managed_server_url().map_err(|error| ServiceHealthError { state: "configuration_error", message: format!("{error:#}") })?;
            managed_admin_status_at(client, &base).await
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiMemoryStatus {
    pub installed: bool,
    pub executable: Option<String>,
    pub install_source: Option<String>,
    pub managed_install_available: bool,
    pub server_url: String,
    pub server_running: bool,
    pub health_state: &'static str,
    pub health_error: Option<String>,
    pub can_start: bool,
    pub version: Option<String>,
    pub counts: Option<Value>,
    pub data_dir: Option<String>,
    pub web_url: Option<String>,
    #[serde(rename = "managedByCrewVela")]
    pub managed_by_crewvela: bool,
}

#[tauri::command]
pub async fn ai_memory_status() -> Result<AiMemoryStatus, String> {
    status().await.map_err(|error| error.to_string())
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SetupAgent { ClaudeCode, Codex, KimiCode, Zcode }

impl SetupAgent {
    fn id(self) -> &'static str {
        match self { Self::ClaudeCode => "claude-code", Self::Codex => "codex", Self::KimiCode => "kimi-code", Self::Zcode => "zcode" }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SetupAction { Hooks, Mcp }

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetupResult {
    success: bool,
    exit_code: Option<i32>,
    timed_out: bool,
    stdout: String,
    stderr: String,
    output_truncated: bool,
}

fn setup_command(executable: &Path, data: &Path, url: &str, agent: SetupAgent, action: SetupAction, project: Option<&str>) -> Result<Command> {
    let mut command = Command::new(executable);
    command.arg("--data-dir").arg(data);
    match action {
        SetupAction::Hooks => { command.args(["install-hooks", "--agent", agent.id()]); }
        SetupAction::Mcp => { command.args(["install-mcp", "--client", agent.id()]); }
    }
    command.args(["--server-url", url, "--apply"]);
    if matches!((agent, action), (SetupAgent::ClaudeCode, SetupAction::Hooks)) {
        let selected = PathBuf::from(project.context("请先选择 Claude 钩子的目标项目目录")?);
        if !selected.is_absolute() { bail!("项目目录必须是绝对路径"); }
        let directory = selected.canonicalize().context("项目目录不存在或不可访问")?;
        if !directory.is_dir() { bail!("目标项目必须是目录"); }
        command.args(["--scope", "project"]).current_dir(directory);
    } else {
        // User-level configuration must not inherit an unrelated process cwd.
        command.current_dir(data);
    }
    command.creation_flags_no_window();
    auth::command(&mut command, data, url)?;
    Ok(command)
}

/// Drain both pipes concurrently; retain only a bounded prefix while continuing
/// to read, so verbose commands cannot deadlock or exhaust memory.
fn read_setup_output(mut pipe: impl std::io::Read + Send + 'static) -> std::sync::mpsc::Receiver<std::io::Result<(String, bool)>> {
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let mut saved = Vec::new();
            let mut buffer = [0u8; 4096];
            let mut truncated = false;
            loop {
                let count = pipe.read(&mut buffer)?;
                if count == 0 { break; }
                let keep = count.min(65536usize.saturating_sub(saved.len()));
                saved.extend_from_slice(&buffer[..keep]);
                truncated |= keep < count;
            }
            Ok((String::from_utf8_lossy(&saved).into_owned(), truncated))
        })();
        let _ = sender.send(result);
    });
    receiver
}

fn scrub_setup_output(mut text: String, secret: Option<&str>, truncated: bool) -> String {
    if let Some(secret) = secret.filter(|value| !value.is_empty()) {
        // The bounded output reader can stop part-way through a token.
        // Remove an incomplete token at that boundary as well as full matches.
        if truncated {
            for (length, _) in secret.char_indices().rev() {
                if length > 0 && text.ends_with(&secret[..length]) {
                    text.truncate(text.len() - length);
                    text.push_str("[访问令牌已隐藏]");
                    break;
                }
            }
        }
        text = text.replace(secret, "[访问令牌已隐藏]");
    }
    text
}

fn run_setup_command(mut command: Command, timeout: Duration) -> Result<SetupResult> {
    let secret = command.get_envs().find(|(k,_)| *k == "AI_MEMORY_AUTH_TOKEN").and_then(|(_,v)| v).map(|s| s.to_string_lossy().into_owned())
        .or_else(|| std::env::var("AI_MEMORY_AUTH_TOKEN").ok());
    command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).stdin(std::process::Stdio::null());
    let mut child = command.spawn().context("启动配置命令失败")?;
    let stdout = read_setup_output(child.stdout.take().context("无法读取标准输出")?);
    let stderr = read_setup_output(child.stderr.take().context("无法读取错误输出")?);
    let started = std::time::Instant::now();
    let (exit, timed_out) = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break (exit, false),
            Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
            Ok(None) => { child.kill().context("配置命令超时且终止失败")?; break (child.wait()?, true); }
            Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error.into()); }
        }
    };
    let (out, out_truncated) = stdout.recv_timeout(Duration::from_secs(2)).context("标准输出未在限定时间内关闭")??;
    let (err, err_truncated) = stderr.recv_timeout(Duration::from_secs(2)).context("错误输出未在限定时间内关闭")??;
    Ok(SetupResult { success: exit.success() && !timed_out, exit_code: exit.code(), timed_out, stdout: scrub_setup_output(out,secret.as_deref(),out_truncated), stderr: scrub_setup_output(err,secret.as_deref(),err_truncated), output_truncated: out_truncated || err_truncated })
}

static SETUP_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn guard_project_registration(path: &Path, action: SetupAction) -> Result<()> {
    let Some(config) = verification::read_config(path)? else { return Ok(()); };
    fn contains_script(value: &Value, name: &str) -> bool {
        match value {
            Value::String(text) => text.contains(name),
            Value::Array(values) => values.iter().any(|value| contains_script(value, name)),
            Value::Object(values) => values.values().any(|value| contains_script(value, name)),
            _ => false,
        }
    }
    let script = match action { SetupAction::Hooks => "ai-memory-project-hook.ps1", SetupAction::Mcp => "ai-memory-mcp-bridge.mjs" };
    let locations: &[&str] = match action {
        SetupAction::Hooks => &["/hooks"],
        SetupAction::Mcp => &["/mcpServers/ai-memory", "/mcp_servers/ai-memory", "/mcp/servers/ai-memory"],
    };
    if locations.iter().filter_map(|p| config.pointer(p)).any(|entry| contains_script(entry, script)) {
        bail!("当前接入使用项目范围保护脚本 {script}。请保留该配置并执行接入检查；覆盖安装可能取消现有项目边界");
    }
    Ok(())
}

#[tauri::command]
pub async fn ai_memory_setup(agent: SetupAgent, action: SetupAction, project_dir: Option<String>) -> Result<SetupResult, String> {
    onboarding::require_idle()?;
    tokio::task::spawn_blocking(move || -> Result<SetupResult> {
        let root=crate::providers::config_dir()?;
        let _operation=updates::operation_lock(&root)?;
        if updates::pending(&root) { bail!("上次更新未完成，请先恢复升级前快照"); }
        let _guard = SETUP_LOCK.try_lock().map_err(|_| anyhow::anyhow!("另一个接入配置正在执行，请稍后重试"))?;
        let (executable, _) = locate_executable().context("未找到 ai-memory，请先安装")?;
        let data = data_dir()?;
        if !data.is_dir() { bail!("共享记忆数据目录不存在，请先启动服务初始化"); }
        let project = project_dir.as_deref().map(Path::new).unwrap_or_else(|| Path::new("."));
        let (mcp_config, hooks_config) = verification::config_paths(agent, project)?;
        guard_project_registration(match action { SetupAction::Hooks => &hooks_config, SetupAction::Mcp => &mcp_config }, action)?;
        let command = setup_command(&executable, &data, &resolved_server_url()?, agent, action, project_dir.as_deref())?;
        run_setup_command(command, Duration::from_secs(60))
    }).await.map_err(|error| error.to_string())?.map_err(|error| format!("{error:#}"))
}

async fn status() -> Result<AiMemoryStatus> {
    let client = service_client(&resolved_server_url().unwrap_or_default())?;
    let health = admin_status(&client).await;
    let health_state = health.as_ref().err().map_or("ready", |error| error.state);
    let health_error = health.as_ref().err().map(ToString::to_string);
    let running = health.ok();
    let (executable, source) = match locate_executable() {
        Some((path, source)) => (Some(path.display().to_string()), Some(source.to_string())),
        None => (None, None),
    };
    let managed_by_crewvela = managed_service()?.is_running()?;
    let can_start = health_state == "offline" && server_url_override().is_none() && !managed_by_crewvela;
    let server = running.as_ref();
    Ok(AiMemoryStatus {
        installed: executable.is_some(),
        executable,
        install_source: source,
        managed_install_available: installation::current_package().is_some(),
        server_url: resolved_server_url().unwrap_or_default(),
        server_running: server.is_some(),
        health_state,
        health_error,
        can_start,
        version: server
            .and_then(|value| value.get("version"))
            .and_then(Value::as_str)
            .map(str::to_string),
        counts: server.and_then(|value| value.get("counts")).cloned(),
        data_dir: data_dir().ok().map(|path| path.display().to_string()),
        web_url: running.as_ref().and_then(|_| resolved_server_url().ok()).map(|url| format!("{url}/web")),
        managed_by_crewvela,
    })
}

/// 校验并安装官方稳定版；更新已有安装需显式确认，数据目录保持独立。
#[tauri::command]
pub async fn ai_memory_install() -> Result<AiMemoryStatus, String> {
    onboarding::require_idle()?;
    install().await.map_err(|error| error.to_string())
}

async fn install() -> Result<AiMemoryStatus> {
    let _operation = updates::operation_lock(&crate::providers::config_dir()?)?;
    let _guard = service_lifecycle_lock(Duration::from_secs(20)).await?;
    if updates::pending(&crate::providers::config_dir()?) { bail!("上次更新未完成，请先恢复升级前快照"); }
    installation::install(&managed_dir()?, None).await?;
    status().await
}

/// 启动服务（幂等）：先 init 数据目录，再 detached 拉起 serve，轮询确认。
#[tauri::command]
pub async fn ai_memory_serve() -> Result<AiMemoryStatus, String> {
    onboarding::require_idle()?;
    serve().await.map_err(|error| format!("{error:#}"))
}

async fn serve() -> Result<AiMemoryStatus> {
    let _operation = updates::operation_lock(&crate::providers::config_dir()?)?;
    if updates::pending(&crate::providers::config_dir()?) { bail!("上次更新未完成，请先恢复升级前快照，未继续启动可能已迁移的数据"); }
    let _lifecycle = service_lifecycle_lock(Duration::from_secs(20)).await?;
    let client = service_client(&resolved_server_url().unwrap_or_default())?;
    match admin_status(&client).await {
        Ok(_) => return status().await,
        Err(error) if error.state == "offline" && server_url_override().is_none() => {},
        Err(error) => return Err(error).context("无法启动记忆服务，请先处理当前监听地址的异常"),
    }
    if managed_service()?.is_running()? {
        bail!("托管 ai-memory 进程仍在运行但服务未响应，请先停止服务后重试");
    }
    let (executable, _) = locate_executable().context("未找到 ai-memory 可执行文件，请先安装")?;
    let data = data_dir()?;
    start_service_at(&executable, &data, &resolved_server_url()?).await?;
    status().await
}

/// Shared by manual startup and the first-use workflow; callers hold the lifecycle lock.
async fn start_service_at(executable: &Path, data: &Path, base: &str) -> Result<()> {
    let client = service_client(base)?;
    match managed_status_for_data(&client, base, data).await {
        Ok(_) => return Ok(()),
        Err(error) if error.state == "offline" => {},
        Err(error) => return Err(error).context("记忆服务地址异常，未替换现有监听服务"),
    }
    if managed_service()?.is_running()? { bail!("已有托管进程未就绪，未启动替代进程"); }
    std::fs::create_dir_all(&data)
        .with_context(|| format!("创建数据目录失败: {}", data.display()))?;
    // init 幂等：仅补齐目录布局与默认配置，不触碰已有数据。current_dir
    // 固定在数据目录，默认 config.toml 就落在那里（2.6.0 写到相对路径）。
    let mut init = Command::new(&executable);
    init
        .arg("--data-dir")
        .arg(&data)
        .arg("init")
        .current_dir(&data)
        .creation_flags_no_window();
    run_memory_init(init, Duration::from_secs(10))?;
    let mut command = Command::new(&executable);
    command
        .arg("--data-dir")
        .arg(&data)
        .arg("serve")
        .arg("--transport")
        .arg("http")
        .arg("--enable-web")
        // The independent service must not keep the launcher's/test runner's
        // pipes open. ai-memory writes diagnostics to its own data/logs files.
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let config = data.join("config.toml");
    if config.is_file() {
        command.arg("--config").arg(config);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：无控制台闪窗；CREATE_NEW_PROCESS_GROUP：独立进程组，
        // 不随本应用的作业对象 / Ctrl+C 传播退出。
        command.creation_flags(0x0800_0000 | 0x0000_0200);
    }
    #[cfg(target_os = "macos")]
    {
        use std::os::unix::process::CommandExt;
        command.current_dir(data).process_group(0);
    }
    let child = command
        .spawn()
        .with_context(|| format!("启动 ai-memory 服务失败: {}", executable.display()))?;
    let pid = child.id();
    managed_service()?.child = Some(child);
    // 就绪阶段总预算15s，包含每次HTTP请求与轮询间隔；不含此前init/排队。
    wait_for_service_startup(
        Duration::from_secs(15),
        || managed_service()?.is_running(),
        || managed_status_for_data(&client, base, data),
        || {
            let mut service = managed_service()?;
            if service.is_running()? { service.stop()?; }
            Ok(())
        },
    ).await.with_context(|| format!("ai-memory 服务启动失败（pid {pid}），请查看健康检查及进程清理结果"))?;
    Ok(())
}

#[tauri::command]
pub async fn ai_memory_stop() -> Result<AiMemoryStatus, String> {
    onboarding::require_idle()?;
    stop().await.map_err(|error| error.to_string())
}

async fn stop() -> Result<AiMemoryStatus> {
    let _operation = updates::operation_lock(&crate::providers::config_dir()?)?;
    let _lifecycle = service_lifecycle_lock(Duration::from_secs(20)).await?;
    managed_service()?.stop()?;
    status().await
}

#[allow(dead_code)]
trait CommandExtNoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}

#[cfg(windows)]
impl CommandExtNoWindow for Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        self.creation_flags(0x0800_0000);
        self
    }
}

#[cfg(not(windows))]
impl CommandExtNoWindow for Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenHandoffInfo {
    pub id: String,
    pub from_agent: String,
    pub created_at_ms: i64,
}

/// 按会话目录查询 open handoffs（只读；领取由智能体自身的 SessionStart 完成，
/// CrewVela 只展示"有交接棒待接"这一事实——offer-not-claim）。
#[tauri::command]
pub async fn ai_memory_open_handoffs(
    cwds: Vec<String>,
) -> Result<std::collections::HashMap<String, Vec<OpenHandoffInfo>>, String> {
    open_handoffs(cwds).await.map_err(|error| format!("{error:#}"))
}

async fn open_handoffs(
    cwds: Vec<String>,
) -> Result<std::collections::HashMap<String, Vec<OpenHandoffInfo>>> {
    let client = service_client(&resolved_server_url().unwrap_or_default())?;
    admin_status(&client).await.context("无法查询共享记忆交接记录")?;
    let (executable, _) = locate_executable().context("未找到 ai-memory，无法解析项目的记忆作用域")?;
    let data = data_dir()?;
    let mut by_scope: std::collections::HashMap<(String, String), Vec<String>> = Default::default();
    // Resolver performs filesystem/process IO off the async runtime. Deduplicate
    // cwd inputs; an invalid marker must never fall back to another project's scope.
    for cwd in cwds.into_iter().collect::<std::collections::HashSet<_>>() {
        let exe = executable.clone();
        let data = data.clone();
        let path = cwd.clone();
        let scope = tokio::task::spawn_blocking(move || inspect_scope(&exe, &data, &path))
            .await.context("解析项目共享记忆作用域的任务失败")?
            .with_context(|| format!("无法确认项目 {cwd} 的共享记忆作用域，请检查项目标记和捕获策略"))?;
        by_scope.entry(scope).or_default().push(cwd);
    }
    let mut result = std::collections::HashMap::new();
    for ((workspace, project), paths) in by_scope {
        let handoffs = query_handoffs_at(&client, &resolved_server_url()?, &workspace, &project).await?;
        for path in paths {
            let infos = handoffs
                .iter()
                .filter(|item| handoff_matches_cwd(item, &path))
                .filter_map(|item| {
                    Some(OpenHandoffInfo {
                        id: item.get("id")?.as_str()?.to_string(),
                        from_agent: item.get("from_agent")?.as_str()?.to_string(),
                        created_at_ms: item.get("created_at_ms")?.as_i64()?,
                    })
                })
                .collect();
            result.insert(path, infos);
        }
    }
    Ok(result)
}

async fn query_handoffs_at(client: &reqwest::Client, base: &str, workspace: &str, project: &str) -> Result<Vec<Value>> {
    let response = authenticated_request(client, format!("{}/admin/handoffs", base.trim_end_matches('/')))?
        .query(&[("workspace", workspace), ("project", project), ("limit", "100")])
        .timeout(Duration::from_secs(3));
    let response = response.send().await.map_err(service_transport_error).context("读取共享记忆交接记录失败")?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        let body: Value = response.json().await.context("共享记忆交接查询返回 HTTP 404，错误响应无法解析，未确认记录为空")?;
        let missing_workspace = format!("workspace '{workspace}' not found");
        let missing_project = format!("project '{project}' not found in workspace '{workspace}'");
        if body.get("error").and_then(Value::as_str)
            .is_some_and(|message| message == missing_workspace || message == missing_project)
        {
            // A newly opened project need not have any captured memory yet.
            // No scopes/records are created or claimed by this read-only check.
            return Ok(Vec::new());
        }
        bail!("共享记忆交接查询返回 HTTP 404，未确认项目或工作区不存在");
    }
    let response = service_response_json(response).await.context("读取共享记忆交接记录失败")?;
    Ok(parse_handoffs(&response)?.to_vec())
}

fn parse_handoffs(response: &Value) -> Result<&[Value]> {
    let handoffs = response.get("handoffs").and_then(Value::as_array)
        .context("记忆服务交接响应缺少 handoffs 数组")?;
    for item in handoffs {
        if item.get("id").and_then(Value::as_str).is_none()
            || item.get("from_agent").and_then(Value::as_str).is_none()
            || item.get("created_at_ms").and_then(Value::as_i64).is_none()
            || item.get("cwd").is_some_and(|cwd| !cwd.is_null() && !cwd.is_string()) {
            bail!("记忆服务交接记录格式不完整");
        }
    }
    Ok(handoffs)
}

/// The installed CLI owns marker parsing, home routing and rejection policy.
/// --check-capture neither spools events nor claims handoffs.
fn inspect_scope(executable: &Path, data: &Path, cwd: &str) -> Option<(String, String)> {
    inspect_scope_at(executable, data, cwd, &resolved_server_url().ok()?)
}

fn inspect_scope_at(executable: &Path, data: &Path, cwd: &str, base: &str) -> Option<(String, String)> {
    let mut command = Command::new(executable);
    command
        .args([
            "--data-dir",
            data.to_str()?,
            "hook",
            "--agent",
            "codex",
            "--event",
            "session-start",
            "--check-capture",
            "--server-url",
            base,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command.spawn().ok()?;
    let payload = serde_json::json!({"cwd": cwd, "session_id": "crewvela-read-only-scope"});
    let written = child
        .stdin
        .take()?
        .write_all(payload.to_string().as_bytes());
    if written.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() < Duration::from_secs(3) => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    scope_from_report(&serde_json::from_slice::<Value>(&output.stdout).ok()?, cwd)
}

fn scope_from_report(report: &Value, cwd: &str) -> Option<(String, String)> {
    // Named profiles can target another server, whose URL is intentionally not
    // exposed by this inspection. Suppress badges instead of querying ours.
    if report["server_resolution"] != "install-default" || report["policy_state"] == "invalid"
        || report["admits_capture"] == Value::Bool(false) {
        return None;
    }
    match report["scope_resolution"].as_str()? {
        "explicit" => {
            let workspace = report["scope"]["workspace"].as_str()?.trim();
            let project = report["scope"]["project"].as_str()?.trim();
            if workspace.is_empty() || project.is_empty() {
                return None;
            }
            Some((workspace.to_owned(), project.to_owned()))
        }
        "server-derived" => Some(("default".into(), project_name(cwd))),
        _ => None,
    }
}

fn handoff_matches_cwd(item: &Value, cwd: &str) -> bool {
    // Admin metadata omits from_session_id, so cannot distinguish manual from
    // automatic handoffs. Conservatively path-bound all cwd-bearing rows.
    let Some(origin) = item["cwd"].as_str() else {
        return true;
    };
    let normalize = |path: &str| {
        let path = path.replace('\\', "/").trim_end_matches('/').to_owned();
        if cfg!(windows) {
            path.to_lowercase()
        } else {
            path
        }
    };
    let origin = normalize(origin);
    let receiving = normalize(cwd);
    if origin.is_empty() || receiving.is_empty() {
        return false;
    }
    receiving == origin
        || receiving
            .strip_prefix(&origin)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn project_name(cwd: &str) -> String {
    Path::new(cwd)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| cwd.to_string())
}

/// 供会话列表轻量探测：当前有多少会话目录挂着 open handoffs。
#[tauri::command]
pub async fn ai_memory_session_badges(
    sessions: State<'_, SessionManager>,
) -> Result<std::collections::HashMap<String, Vec<OpenHandoffInfo>>, String> {
    let cwds = sessions
        .list()
        .iter()
        .map(|record| record.cwd.clone())
        .collect::<Vec<_>>();
    open_handoffs(cwds).await.map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn status_serialization_matches_the_crewvela_frontend_contract() {
        let status = super::AiMemoryStatus {
            installed: false, executable: None, install_source: None,
            managed_install_available: false, server_url: "http://127.0.0.1:49374".into(),
            server_running: false, health_state: "offline", health_error: None,
            can_start: true, version: None, counts: None, data_dir: None, web_url: None,
            managed_by_crewvela: true,
        };
        let value = serde_json::to_value(status).unwrap();
        assert_eq!(value["managedByCrewVela"], true);
        assert!(value.get("managedByCrewvela").is_none());
    }

    use super::*;
    use serde_json::json;

    fn http_fixture(status: &str, body: &str, delay: Duration) -> (String, std::thread::JoinHandle<()>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let thread = std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let mut socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && started.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(10)),
                    Err(error) => panic!("HTTP fixture did not receive a request: {error}"),
                }
            };
            // Winsock inherits the listener's nonblocking mode on accept. A read
            // timeout alone does not make fragmented request headers blocking.
            socket.set_nonblocking(false).unwrap();
            socket.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let count = socket.read(&mut chunk).expect("read HTTP request headers");
                assert!(count > 0, "HTTP request closed before its headers completed");
                request.extend_from_slice(&chunk[..count]);
                assert!(request.len() <= 16 * 1024, "fixture request headers exceeded limit");
            }
            std::thread::sleep(delay);
            let _ = socket.write_all(response.as_bytes());
        });
        (url, thread)
    }

    fn pending_http_fixture() -> (String, std::sync::mpsc::Sender<()>, std::thread::JoinHandle<()>) {
        use std::sync::mpsc::{channel, RecvTimeoutError};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (ready, started) = channel();
        let (finish, finished) = channel();
        let thread = std::thread::spawn(move || {
            ready.send(()).unwrap();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                match listener.accept() {
                    Ok((socket, _)) => {
                        // Deliberately never respond. The client may time out before
                        // transmitting headers, which is valid for a total timeout.
                        finished.recv_timeout(Duration::from_secs(5)).expect("timeout probe did not finish");
                        drop(socket);
                        return;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(error) => panic!("timeout fixture accept failed: {error}"),
                }
                match finished.recv_timeout(Duration::from_millis(1)) {
                    Ok(()) => return,
                    Err(RecvTimeoutError::Timeout) if std::time::Instant::now() < deadline => {}
                    Err(error) => panic!("timeout probe did not finish: {error}"),
                }
            }
        });
        started.recv_timeout(Duration::from_secs(2)).expect("timeout fixture did not start");
        (url, finish, thread)
    }

    #[tokio::test]
    async fn handoff_preflight_missing_memory_scope_is_empty() {
        let body = r#"{"error":"project 'new-project' not found in workspace 'default'"}"#;
        let (url, server) = http_fixture("404 Not Found", body, Duration::ZERO);
        let result = query_handoffs_at(&reqwest::Client::new(), &url, "default", "new-project").await;
        server.join().unwrap();
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn handoff_lookup_preserves_authentication_unknown_404_and_schema_errors() {
        for (status, body) in [
            ("401 Unauthorized", r#"{"error":"unauthorized"}"#),
            ("403 Forbidden", r#"{"error":"forbidden"}"#),
            ("500 Internal Server Error", r#"{"error":"storage failure"}"#),
            ("404 Not Found", r#"{"error":"route not found"}"#),
            ("404 Not Found", r#"{"error":"project 'different-project' not found in workspace 'default'"}"#),
            ("404 Not Found", "not json"),
            ("200 OK", r#"{"handoffs":null}"#),
        ] {
            let (url, server) = http_fixture(status, body, Duration::ZERO);
            let result = query_handoffs_at(&reqwest::Client::new(), &url, "default", "new-project").await;
            server.join().unwrap();
            assert!(result.is_err(), "must retain {status}: {body}");
        }
        let (url, server) = http_fixture("404 Not Found", r#"{"error":"workspace 'default' not found"}"#, Duration::ZERO);
        assert!(query_handoffs_at(&reqwest::Client::new(), &url, "default", "new-project").await.unwrap().is_empty());
        server.join().unwrap();
        let (url, server) = http_fixture("200 OK", r#"{"handoffs":[{"id":"baton-1","from_agent":"claude-code","created_at_ms":123,"cwd":"/fixture"}]}"#, Duration::ZERO);
        let records = query_handoffs_at(&reqwest::Client::new(), &url, "default", "new-project").await.unwrap();
        assert_eq!(records[0]["id"], "baton-1");
        server.join().unwrap();
    }

    #[tokio::test]
    #[ignore = "读取本机已安装共享记忆服务；显式传入 CREWVELA_TEST_HANDOFF_CWD，不创建或领取交接棒"]
    async fn handoff_preflight_live_memory_scope_query() {
        let cwd = std::env::var("CREWVELA_TEST_HANDOFF_CWD").expect("explicit diagnostic cwd");
        let result = open_handoffs(vec![cwd.clone()]).await.unwrap();
        let records = result.get(&cwd).expect("the queried cwd must be returned");
        println!("HANDOFF_MEMORY_READ_OK count={} claim=false", records.len());
    }

    #[tokio::test]
    async fn health_distinguishes_http_authentication_invalid_responses_and_timeout() {
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for (status, body, expected) in [
            ("200 OK", r#"{"version":"2.6.0","counts":{}}"#, "ready"),
            ("401 Unauthorized", r#"{}"#, "authentication_error"),
            ("403 Forbidden", r#"{}"#, "authentication_error"),
            ("503 Unavailable", r#"{}"#, "http_error"),
            ("200 OK", "not-json", "invalid_response"),
            ("200 OK", r#"{"message":"another application"}"#, "invalid_response"),
        ] {
            let (url, thread) = http_fixture(status, body, Duration::ZERO);
            let actual = admin_status_at(&client, &url).await;
            thread.join().unwrap();
            assert_eq!(actual.as_ref().err().map_or("ready", |error| error.state), expected);
            if let Err(error) = actual { assert!(!error.message.is_empty()); }
        }
        assert!(connection_was_refused(&std::io::Error::from(std::io::ErrorKind::ConnectionRefused)));
        assert!(!connection_was_refused(&std::io::Error::from(std::io::ErrorKind::PermissionDenied)));
        assert!(!connection_was_refused(&std::io::Error::from(std::io::ErrorKind::TimedOut)));
        let (url, finish, thread) = pending_http_fixture();
        let actual = service_json(client.get(url).timeout(Duration::from_millis(20))).await;
        finish.send(()).unwrap();
        thread.join().unwrap();
        assert_eq!(actual.unwrap_err().state, "timeout");
    }

    #[test]
    fn http_fixture_waits_for_fragmented_request_headers() {
        use std::io::{Read, Write};
        let (url, server) = http_fixture("200 OK", "{}", Duration::ZERO);
        let mut client = std::net::TcpStream::connect(url.trim_start_matches("http://")).unwrap();
        client.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        client.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        client.write_all(b"Connection: close\r\n\r\n").unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.ends_with("\r\n\r\n{}"));
    }

    #[tokio::test]
    async fn unbound_managed_port_is_offline_without_waiting_for_a_firewall_timeout() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        assert!(!managed_listener_is_unbound(&base).unwrap(), "an occupied port must not be treated as offline");
        drop(listener);
        assert!(managed_listener_is_unbound(&base).unwrap());
        let client = service_client(&base).unwrap();
        let started = std::time::Instant::now();
        let error = managed_admin_status_at(&client, &base).await.unwrap_err();
        assert_eq!(error.state, "offline");
        assert!(started.elapsed() < Duration::from_secs(1), "listener absence must not wait for the HTTP/connect timeout");
        for external in ["https://memory.example:47374", "http://memory.example:47374", "http://192.0.2.1:47374", "http://0.0.0.0:47374"] {
            assert!(!managed_listener_is_unbound(external).unwrap(), "external endpoint {external} must not be probed by local bind");
        }
    }

    #[tokio::test]
    async fn bound_managed_listener_keeps_auth_http_and_timeout_errors() {
        let client = service_client(DEFAULT_SERVER_URL).unwrap();
        for (status, body, delay, expected) in [
            ("401 Unauthorized", "{}", Duration::ZERO, "authentication_error"),
            ("503 Unavailable", "{}", Duration::ZERO, "http_error"),
            ("200 OK", r#"{"version":"2.6.0","counts":{}}"#, Duration::from_millis(2100), "timeout"),
        ] {
            let (base, thread) = http_fixture(status, body, delay);
            assert!(!managed_listener_is_unbound(&base).unwrap());
            let error = managed_admin_status_at(&client, &base).await.unwrap_err();
            assert_eq!(error.state, expected);
            thread.join().unwrap();
        }
    }

    #[tokio::test]
    async fn local_service_bypasses_outbound_proxy_but_remote_keeps_it() {
        let body = r#"{"version":"2.6.0","counts":{}}"#;
        let (base, thread) = http_fixture("200 OK", body, Duration::ZERO);
        let broken_proxy = reqwest::Proxy::all("http://127.0.0.1:1").unwrap();
        let client = configure_service_client(&base, reqwest::Client::builder().proxy(broken_proxy)).build().unwrap();
        assert!(admin_status_at(&client, &base).await.is_ok());
        thread.join().unwrap();

        let (proxy, thread) = http_fixture("200 OK", body, Duration::ZERO);
        let base = "http://memory.example:47374";
        let client = configure_service_client(base, reqwest::Client::builder().proxy(reqwest::Proxy::all(proxy).unwrap())).build().unwrap();
        assert!(admin_status_at(&client, base).await.is_ok(), "remote endpoint must retain the explicitly configured outbound proxy");
        thread.join().unwrap();
    }

    #[tokio::test]
    async fn startup_budget_includes_pending_health_and_kills_only_the_owned_child() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "ai_memory::tests::setup_process_fixture", "--nocapture"])
            .env("CREWVELA_SETUP_TEST_FIXTURE", "timeout").creation_flags_no_window();
        let owned = std::cell::RefCell::new(ManagedService { child: Some(command.spawn().unwrap()) });
        let started = std::time::Instant::now();
        let error = wait_for_service_startup(
            Duration::from_millis(80),
            || owned.borrow_mut().is_running(),
            || async { tokio::time::sleep(Duration::from_secs(5)).await; Ok(json!({"version":"fixture","counts":{}})) },
            || owned.borrow_mut().stop(),
        ).await.unwrap_err();
        assert!(format!("{error:#}").contains("总时限"));
        assert!(started.elapsed() < Duration::from_secs(1), "one pending health request must not exceed the startup budget");
        assert!(!owned.borrow_mut().is_running().unwrap(), "only this fixture's owned process must be reaped on failure");
    }

    #[tokio::test]
    async fn startup_budget_preserves_health_failure_and_cleanup_failure() {
        let cleaned = std::cell::Cell::new(false);
        let error = wait_for_service_startup(
            Duration::from_millis(40),
            || Ok(true),
            || async { Err(ServiceHealthError { state: "authentication_error", message: "fixture HTTP 401".into() }) },
            || { cleaned.set(true); Ok(()) },
        ).await.unwrap_err();
        assert!(cleaned.get());
        assert!(format!("{error:#}").contains("fixture HTTP 401"));

        let error = wait_for_service_startup(
            Duration::from_millis(40),
            || Ok(false),
            || async { panic!("an exited child must not be probed"); #[allow(unreachable_code)] Ok(Value::Null) },
            || bail!("fixture cleanup failure"),
        ).await.unwrap_err();
        let message = format!("{error:#}");
        assert!(message.contains("就绪前退出") && message.contains("fixture cleanup failure"));

        let cleaned = std::cell::Cell::new(false);
        wait_for_service_startup(
            Duration::from_millis(40),
            || Ok(true),
            || async { Ok(json!({"version":"fixture","counts":{}})) },
            || { cleaned.set(true); Ok(()) },
        ).await.unwrap();
        assert!(!cleaned.get(), "successful startup must retain the service");
    }

    #[test]
    fn managed_listener_uses_configured_loopback_port_and_rejects_external_binds() {
        assert_eq!(managed_url_from_config("").unwrap(), DEFAULT_SERVER_URL);
        assert_eq!(managed_url_from_config("bind = '127.0.0.1:47374'").unwrap(), "http://127.0.0.1:47374");
        assert_eq!(managed_url_from_config("bind = '[::1]:47374'").unwrap(), "http://[::1]:47374");
        for config in ["bind = '0.0.0.0:47374'", "bind = '127.0.0.1:0'", "bind = 'not an address'", "broken = ["] {
            assert!(managed_url_from_config(config).is_err(), "{config}");
        }
    }

    #[test]
    fn setup_preserves_existing_project_scope_protection() {
        let directory = std::env::temp_dir().join(format!("memory-scope-guard-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let file = directory.join("settings.json");
        for (action, script) in [(SetupAction::Hooks, "ai-memory-project-hook.ps1"), (SetupAction::Mcp, "ai-memory-mcp-bridge.mjs")] {
            let config = match action {
                SetupAction::Hooks => json!({"hooks": [{"command": "pwsh", "args": [script]}]}),
                SetupAction::Mcp => json!({"mcpServers": {"ai-memory": {"command": "node", "args": [script]}}}),
            };
            let bytes = serde_json::to_vec(&config).unwrap();
            std::fs::write(&file, &bytes).unwrap();
            assert!(guard_project_registration(&file, action).unwrap_err().to_string().contains("项目范围保护"));
            assert_eq!(std::fs::read(&file).unwrap(), bytes);
        }
        std::fs::write(&file, "{}").unwrap();
        assert!(guard_project_registration(&file, SetupAction::Hooks).is_ok());
        std::fs::write(&file, "broken JSON").unwrap();
        assert!(guard_project_registration(&file, SetupAction::Mcp).is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn handoff_lookup_distinguishes_empty_results_from_malformed_responses() {
        assert!(parse_handoffs(&json!({"handoffs": []})).unwrap().is_empty());
        let valid = json!({"id": "one", "from_agent": "codex", "created_at_ms": 1, "cwd": "/repo"});
        assert_eq!(parse_handoffs(&json!({"handoffs": [valid.clone()]})).unwrap().len(), 1);
        for invalid in [Value::Null, json!({}), json!({"handoffs": null}), json!({"handoffs": [{}]})] {
            assert!(parse_handoffs(&invalid).is_err());
        }
        let mut bad_cwd = valid;
        bad_cwd["cwd"] = json!(123);
        assert!(parse_handoffs(&json!({"handoffs": [bad_cwd]})).is_err());
    }

    #[test]
    fn codex_start_recovers_stopped_local_service_and_preserves_external_registrations() {
        let directory = std::env::temp_dir().join(format!("codex-memory-start-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let config = directory.join("config.toml");
        let starts = std::cell::Cell::new(0);
        let running = std::cell::Cell::new(false);
        let ensure = || ensure_codex_service_with(&config, DEFAULT_SERVER_URL, || {
            // Same contract as serve: a running listener is reused, a stopped
            // listener is started only when opening the configured Codex chat.
            if !running.replace(true) { starts.set(starts.get() + 1); }
            Ok(())
        });
        ensure().unwrap();
        assert_eq!(starts.get(), 0, "missing configuration must not start a service");
        std::fs::write(&config, "[mcp_servers.ai-memory]\nurl = 'http://127.0.0.1:49374/mcp'\n").unwrap();
        ensure().unwrap();
        ensure().unwrap();
        assert_eq!(starts.get(), 1);
        running.set(false); // Explicit stop is retained until the next open.
        assert!(!running.get());
        ensure().unwrap();
        assert!(running.get());
        assert_eq!(starts.get(), 2);
        for contents in [
            "[mcp_servers.other]\nurl = 'http://127.0.0.1:49374/mcp'",
            "[mcp_servers.ai-memory]\nenabled = false\nurl = 'http://127.0.0.1:49374/mcp'",
            "[mcp_servers.ai-memory]\ndisabled = true\nurl = 'http://127.0.0.1:49374/mcp'",
            "[mcp_servers.ai-memory]\ncommand = 'ai-memory'",
            "[mcp_servers.ai-memory]\nurl = 'https://memory.example/mcp'",
            "[mcp_servers.ai-memory]\nurl = 'http://127.0.0.1:12345/mcp'",
        ] {
            std::fs::write(&config, contents).unwrap();
            ensure_codex_service_with(&config, DEFAULT_SERVER_URL, || panic!("external/disabled MCP must not be started")).unwrap();
        }
        std::fs::write(&config, "[mcp_servers.ai-memory]\nurl = 'http://127.0.0.1:49374/mcp'").unwrap();
        ensure_codex_service_with(&config, "https://memory.example", || panic!("remote manager URL must not start a local service")).unwrap();
        let error = ensure_codex_service_with(&config, DEFAULT_SERVER_URL, || bail!("fixture startup failure")).unwrap_err();
        assert!(format!("{error:#}").contains("fixture startup failure"));
        std::fs::write(&config, "invalid [ TOML").unwrap();
        assert!(ensure().is_err(), "configuration errors must be exposed");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn service_stop_uses_owned_child_and_ignores_stale_pid_files() {
        let directory = std::env::temp_dir().join(format!("memory-owned-child-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "ai_memory::tests::setup_process_fixture", "--nocapture"])
            .env("CREWVELA_SETUP_TEST_FIXTURE", "timeout")
            .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .creation_flags_no_window();
        let child = command.spawn().unwrap();
        // A stale record referring to an unrelated live process grants no ownership.
        std::fs::write(directory.join("server.pid"), child.id().to_string()).unwrap();
        let mut external = ManagedService::default();
        assert!(!external.is_running().unwrap());
        assert!(external.stop().unwrap_err().to_string().contains("手动停止"));
        let mut owned = ManagedService { child: Some(child) };
        assert!(owned.is_running().unwrap(), "external stop must not terminate another process");
        owned.stop().unwrap();
        assert!(!owned.is_running().unwrap());
        assert!(owned.stop().is_err());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn exited_owned_service_is_not_reported_as_managed() {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args(["--exact", "ai_memory::tests::setup_process_fixture"])
            .env_remove("CREWVELA_SETUP_TEST_FIXTURE")
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
            .creation_flags_no_window();
        let mut child = command.spawn().unwrap();
        child.wait().unwrap();
        let mut owned = ManagedService { child: Some(child) };
        assert!(!owned.is_running().unwrap());
        assert!(owned.child.is_none());
        assert!(owned.stop().is_err());
    }

    #[test]
    fn setup_allowlist_and_scope() {
        assert!(serde_json::from_str::<SetupAgent>("\"codex;calc\"").is_err());
        assert!(serde_json::from_str::<SetupAction>("\"shell\"").is_err());
        let data = std::env::temp_dir();
        let command = setup_command(Path::new("fixture.exe"), &data, "http://localhost/;echo", SetupAgent::Codex, SetupAction::Mcp, Some("not-a-directory")).unwrap();
        let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert_eq!(args, vec!["--data-dir", data.to_str().unwrap(), "install-mcp", "--client", "codex", "--server-url", "http://localhost/;echo", "--apply"]);
        assert_eq!(command.get_current_dir(), Some(data.as_path()));
        assert!(setup_command(Path::new("fixture.exe"), &data, "url", SetupAgent::ClaudeCode, SetupAction::Hooks, None).is_err());
        assert!(setup_command(Path::new("fixture.exe"), &data, "url", SetupAgent::ClaudeCode, SetupAction::Hooks, Some(".")).is_err());
        assert!(setup_command(Path::new("fixture.exe"), &data, "url", SetupAgent::ClaudeCode, SetupAction::Hooks, Some("missing-project-directory")).is_err());
        let project = data.canonicalize().unwrap();
        let command = setup_command(Path::new("fixture.exe"), &data, "url", SetupAgent::ClaudeCode, SetupAction::Hooks, Some(data.to_str().unwrap())).unwrap();
        assert_eq!(command.get_current_dir(), Some(project.as_path()));
        let args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy().into_owned()).collect();
        assert!(args.windows(2).any(|pair| pair == ["--scope", "project"]));
    }

    #[test]
    fn setup_process_fixture() {
        let Ok(mode) = std::env::var("CREWVELA_SETUP_TEST_FIXTURE") else { return; };
        if mode == "timeout" { std::thread::sleep(Duration::from_secs(10)); return; }
        if mode == "secret" {
            let token=std::env::var("AI_MEMORY_AUTH_TOKEN").unwrap();
            println!("{token}"); eprintln!("{token}"); return;
        }
        println!("{}", "x".repeat(70000));
        eprintln!("fixture diagnostic");
        if mode == "failure" { std::process::exit(7); }
    }

    #[test]
    fn setup_output_never_returns_the_child_service_token() {
        assert_eq!(scrub_setup_output("diagnostic fixture-serv".into(),Some("fixture-service-token-only"),true),"diagnostic [访问令牌已隐藏]");
        assert_eq!(scrub_setup_output("diagnostic fixture-serv".into(),None,true),"diagnostic fixture-serv");
        let mut command=Command::new(std::env::current_exe().unwrap());
        command.args(["--exact","ai_memory::tests::setup_process_fixture","--nocapture"])
            .env("CREWVELA_SETUP_TEST_FIXTURE","secret").env("AI_MEMORY_AUTH_TOKEN","fixture-service-token-only");
        let output=run_setup_command(command,Duration::from_secs(10)).unwrap();
        assert!(output.success);
        for stream in [output.stdout,output.stderr] {
            assert!(!stream.contains("fixture-service-token-only"));
            assert!(stream.contains("[访问令牌已隐藏]"));
        }
    }

    #[test]
    fn setup_process_reports_success_failure_and_timeout() {
        for mode in ["success", "failure", "timeout"] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "ai_memory::tests::setup_process_fixture", "--nocapture"]);
            command.env("CREWVELA_SETUP_TEST_FIXTURE", mode).creation_flags_no_window();
            let result = run_setup_command(command, if mode == "timeout" { Duration::from_millis(150) } else { Duration::from_secs(10) }).unwrap();
            assert_eq!(result.success, mode == "success");
            assert_eq!(result.timed_out, mode == "timeout");
            if mode != "timeout" {
                assert!(result.output_truncated);
                assert!(result.stdout.len() <= 65536);
                assert!(result.stderr.contains("fixture diagnostic"));
                assert_eq!(result.exit_code, Some(if mode == "success" { 0 } else { 7 }));
            }
        }
    }

    #[test]
    fn service_init_and_lifecycle_wait_have_bounded_failures() {
        for mode in ["failure", "timeout"] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args(["--exact", "ai_memory::tests::setup_process_fixture", "--nocapture"]);
            command.env("CREWVELA_SETUP_TEST_FIXTURE", mode).creation_flags_no_window();
            let started = std::time::Instant::now();
            let error = run_memory_init(command, if mode == "timeout" { Duration::from_millis(150) } else { Duration::from_secs(10) }).unwrap_err();
            if mode == "timeout" {
                assert!(error.to_string().contains("超时，已终止"));
                assert!(started.elapsed() < Duration::from_secs(5));
            } else {
                assert!(error.to_string().contains("fixture diagnostic"));
            }
        }
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        runtime.block_on(async {
            let guard = SERVICE_LIFECYCLE.lock().await;
            assert!(service_lifecycle_lock(Duration::from_millis(30)).await.unwrap_err().to_string().contains("超时"));
            drop(guard);
            assert!(service_lifecycle_lock(Duration::from_millis(30)).await.is_ok());
        });
    }

    #[test]
    fn explicit_marker_scope_is_used_and_uncertain_routes_are_closed() {
        let valid = json!({"server_resolution":"install-default", "policy_state":"inactive",
            "scope_resolution":"explicit", "scope":{"workspace":"team", "project":"shared"}});
        assert_eq!(
            scope_from_report(&valid, "/repo"),
            Some(("team".into(), "shared".into()))
        );
        for state in ["partial", "unavailable", "unknown"] {
            let mut report = valid.clone();
            report["scope_resolution"] = state.into();
            assert!(scope_from_report(&report, "/repo").is_none());
        }
        for state in ["rejected", "marker", "unknown"] {
            let mut report = valid.clone();
            report["server_resolution"] = state.into();
            assert!(scope_from_report(&report, "/repo").is_none());
        }
        let mut report = valid.clone();
        report["policy_state"] = "invalid".into();
        assert!(scope_from_report(&report, "/repo").is_none());
        report = valid;
        report["scope"]["project"] = " ".into();
        assert!(scope_from_report(&report, "/repo").is_none());
        assert_eq!(
            scope_from_report(
                &json!({"server_resolution":"install-default",
            "scope_resolution":"server-derived"}),
                "/repo"
            ),
            Some(("default".into(), "repo".into()))
        );
    }

    #[test]
    fn handoff_paths_do_not_leak_between_same_named_directories() {
        let row = json!({"cwd":"/checkout/repo"});
        assert!(handoff_matches_cwd(&row, "/checkout/repo"));
        assert!(handoff_matches_cwd(&row, "/checkout/repo/src"));
        assert!(!handoff_matches_cwd(&row, "/checkout/repository"));
        assert!(!handoff_matches_cwd(&row, "/another/repo"));
        assert!(handoff_matches_cwd(&json!({"cwd":null}), "/another/repo"));
    }

    #[cfg(windows)]
    #[test]
    fn path_lookup_accepts_an_existing_exe_extension() {
        let directory =
            std::env::temp_dir().join(format!("crewvela-which-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("ai-memory.exe"), b"test").unwrap();
        let found =
            which::which_in("ai-memory.exe", Some(directory.as_os_str()), &directory).unwrap();
        assert_eq!(found, directory.join("ai-memory.exe"));
        std::fs::remove_file(found).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }
}
