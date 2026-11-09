// 通用设置（对标 ZCode「常规」设置中的五项刚需，2026-10 调研结论）：
// 网络代理三件套（模型请求 / MCP / 子进程出口）、关闭窗口时隐藏到托盘、
// 保持电脑运行（keep_awake 模块）、终端字体、WebView2 硬件加速开关。
//
// 代理语义（与 ZCode 对齐并做迁移友好处理）：
// - 配置了代理：显式覆盖所有 reqwest 客户端出口，并把 HTTP_PROXY/HTTPS_PROXY/
//   NO_PROXY/NODE_EXTRA_CA_CERTS 写进本进程 env —— 所有后续 spawn 的 ACP 适配器、
//   PTY 会话、MCP 子进程自动继承（CLEAR_VARS 不含代理变量，pty.rs 不剥它们）。
// - 未配置代理：不干预，沿用系统环境变量（保持既有行为不变）。
// - 回滚语义：清空配置时只移除「仍是本应用上次写入值」的变量，
//   避免误删用户自己在系统层设置的代理。
// - localhost/127.0.0.1/::1 永远直连：协作桥、ACP 控制面等本机回环
//   绝不能被代理截走。

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

const SETTINGS_FILE: &str = "general-settings.json";

/// 永远直连的本机回环地址（合并进 no_proxy，保护协作桥/控制面）。
pub(crate) const LOCALHOST_NO_PROXY_ENTRIES: [&str; 3] = ["localhost", "127.0.0.1", "::1"];

fn default_true() -> bool {
    true
}

fn default_terminal_font_size() -> u32 {
    12
}

pub(crate) const MIN_TERMINAL_FONT_SIZE: u32 = 8;
pub(crate) const MAX_TERMINAL_FONT_SIZE: u32 = 28;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettings {
    /// HTTP(S) 代理地址，空 = 沿用系统环境变量。
    #[serde(default)]
    pub http_proxy: String,
    /// 不使用代理的地址（逗号分隔，支持通配）；本机回环会隐式并入。
    #[serde(default)]
    pub http_no_proxy: String,
    /// PEM 根证书路径：注入 reqwest 根信任 + NODE_EXTRA_CA_CERTS（node 系适配器）。
    #[serde(default)]
    pub http_ca_cert_path: String,
    /// 关闭主窗口时隐藏到托盘而不是退出（会话继续运行）。
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    /// 有活动会话或已启用的自动化任务时阻止系统空闲休眠。
    #[serde(default = "default_true")]
    pub keep_awake: bool,
    /// 终端字体栈，空 = 默认 "Cascadia Code", Consolas, monospace。
    #[serde(default)]
    pub terminal_font_family: String,
    #[serde(default = "default_terminal_font_size")]
    pub terminal_font_size: u32,
    /// WebView2 硬件加速；关闭用于规避白屏/花屏/渲染异常，重启生效。
    #[serde(default = "default_true")]
    pub webview2_hardware_acceleration: bool,
}

impl Default for GeneralSettings {
    fn default() -> Self {
        Self {
            http_proxy: String::new(),
            http_no_proxy: String::new(),
            http_ca_cert_path: String::new(),
            close_to_tray: true,
            keep_awake: true,
            terminal_font_family: String::new(),
            terminal_font_size: default_terminal_font_size(),
            webview2_hardware_acceleration: true,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneralSettingsStatus {
    pub settings: GeneralSettings,
    /// 代理 env 是否已写入本进程（新会话立即继承）。
    pub proxy_env_active: bool,
    /// keep-awake 当前是否实际阻止休眠；null = 非 Windows 或轮询未启动。
    pub keep_awake_active: Option<bool>,
    pub warning: Option<String>,
}

// ---- 持久化 ----

fn settings_path_at(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE)
}

pub(crate) fn load_settings_at(path: &Path) -> Result<GeneralSettings> {
    if !path.exists() {
        return Ok(GeneralSettings::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("读取通用设置失败: {}", path.display()))?;
    serde_json::from_str(&raw).context("解析 general-settings.json 失败")
}

fn save_settings_at(path: &Path, settings: &GeneralSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(settings).context("序列化通用设置失败")?,
    )?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

fn load() -> GeneralSettings {
    let path = match crate::providers::config_dir() {
        Ok(dir) => settings_path_at(&dir),
        Err(_) => return GeneralSettings::default(),
    };
    load_settings_at(&path).unwrap_or_default()
}

// ---- 进程内缓存（reqwest 客户端构造点高频读取） ----

static CACHE: RwLock<Option<Arc<GeneralSettings>>> = RwLock::new(None);

/// 读取通用设置（首次访问从磁盘加载，之后走缓存；保存时刷新）。
pub fn load_cached() -> Arc<GeneralSettings> {
    if let Some(cached) = CACHE.read().unwrap().clone() {
        return cached;
    }
    let loaded = Arc::new(load());
    *CACHE.write().unwrap() = Some(loaded.clone());
    loaded
}

fn refresh_cache(settings: &GeneralSettings) {
    *CACHE.write().unwrap() = Some(Arc::new(settings.clone()));
}

// ---- 校验 ----

pub(crate) fn validate_at(settings: &GeneralSettings) -> Result<()> {
    let proxy = settings.http_proxy.trim();
    if !proxy.is_empty() {
        let parsed = reqwest::Url::parse(proxy).map_err(|_| {
            anyhow::anyhow!("代理地址必须是合法 URL，例如 http://127.0.0.1:7890")
        })?;
        if !matches!(parsed.scheme(), "http" | "https") {
            anyhow::bail!("代理地址仅支持 http:// 或 https://（socks 未启用）");
        }
    }
    let ca = settings.http_ca_cert_path.trim();
    if !ca.is_empty() {
        let path = Path::new(ca);
        if !path.is_absolute() {
            anyhow::bail!("自定义证书路径必须是绝对路径");
        }
        if !path.is_file() {
            anyhow::bail!("自定义证书文件不存在：{ca}");
        }
    }
    if !(MIN_TERMINAL_FONT_SIZE..=MAX_TERMINAL_FONT_SIZE).contains(&settings.terminal_font_size) {
        anyhow::bail!(
            "终端字号必须在 {MIN_TERMINAL_FONT_SIZE}-{MAX_TERMINAL_FONT_SIZE} 之间"
        );
    }
    Ok(())
}

fn ca_warning(settings: &GeneralSettings) -> Option<String> {
    let ca = settings.http_ca_cert_path.trim();
    if ca.is_empty() {
        return None;
    }
    match std::fs::read(ca) {
        Ok(pem) if reqwest::Certificate::from_pem(&pem).is_ok() => None,
        Ok(_) => Some("自定义证书不是有效的 PEM 格式，模型请求将忽略它".into()),
        Err(_) => Some(format!("自定义证书读取失败：{ca}")),
    }
}

// ---- no_proxy 合并 ----

/// 用户 no_proxy + 本机回环（去重、大小写不敏感）。
pub(crate) fn effective_no_proxy(raw: &str) -> String {
    let mut parts: Vec<String> = raw
        .split(',')
        .map(|token| token.trim().to_string())
        .filter(|token| !token.is_empty())
        .collect();
    for entry in LOCALHOST_NO_PROXY_ENTRIES {
        if !parts
            .iter()
            .any(|part| part.eq_ignore_ascii_case(entry))
        {
            parts.push(entry.to_string());
        }
    }
    parts.join(",")
}

// ---- reqwest 客户端统一出口 ----

/// 给所有 reqwest ClientBuilder 套上通用设置里的代理与根证书。
/// 未配置代理时不干预（reqwest 默认读系统 env，保持既有行为）。
pub(crate) fn configure_http_client(
    builder: reqwest::ClientBuilder,
) -> reqwest::ClientBuilder {
    let settings = load_cached();
    let mut builder = builder;
    let proxy = settings.http_proxy.trim();
    if !proxy.is_empty() {
        if let Ok(mut upstream) = reqwest::Proxy::all(proxy.to_string()) {
            if let Some(no_proxy) =
                reqwest::NoProxy::from_string(&effective_no_proxy(&settings.http_no_proxy))
            {
                upstream = upstream.no_proxy(Some(no_proxy));
            }
            builder = builder.proxy(upstream);
        }
    }
    let ca_path = settings.http_ca_cert_path.trim();
    if !ca_path.is_empty() {
        if let Ok(pem) = std::fs::read(ca_path) {
            if let Ok(cert) = reqwest::Certificate::from_pem(&pem) {
                builder = builder.add_root_certificate(cert);
            }
        }
    }
    builder
}

// ---- 进程 env 计划（纯函数便于单测） ----

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EnvAction {
    Set(String),
    /// 仅当变量当前值仍等于该值时才移除（回滚我们写过的值，不动用户的）。
    RemoveIfValueIs(String),
}

/// 计算要让本进程 env 反映 settings 所需的动作序列。
/// last_applied = 本应用此前写入的值（用于安全回滚）。
pub(crate) fn plan_process_env(
    settings: &GeneralSettings,
    last_applied: &HashMap<&'static str, String>,
) -> Vec<(&'static str, EnvAction)> {
    let mut plan = Vec::new();
    let proxy = settings.http_proxy.trim().to_string();
    let ca = settings.http_ca_cert_path.trim().to_string();
    let proxy_vars: [&'static str; 3] = ["HTTP_PROXY", "HTTPS_PROXY", "NO_PROXY"];
    if !proxy.is_empty() {
        for name in proxy_vars {
            if name == "NO_PROXY" {
                plan.push((
                    name,
                    EnvAction::Set(effective_no_proxy(&settings.http_no_proxy)),
                ));
            } else {
                plan.push((name, EnvAction::Set(proxy.clone())));
            }
        }
    } else {
        for name in proxy_vars {
            if let Some(previous) = last_applied.get(name) {
                plan.push((name, EnvAction::RemoveIfValueIs(previous.clone())));
            }
        }
    }
    if !ca.is_empty() {
        plan.push(("NODE_EXTRA_CA_CERTS", EnvAction::Set(ca)));
    } else if let Some(previous) = last_applied.get("NODE_EXTRA_CA_CERTS") {
        plan.push((
            "NODE_EXTRA_CA_CERTS",
            EnvAction::RemoveIfValueIs(previous.clone()),
        ));
    }
    plan
}

static LAST_APPLIED_ENV: Mutex<Option<HashMap<&'static str, String>>> = Mutex::new(None);

/// 把通用设置写进本进程 env（子进程继承面）。启动与保存时调用。
pub fn apply_process_env(settings: &GeneralSettings) {
    let mut guard = LAST_APPLIED_ENV.lock().unwrap();
    let last = guard.clone().unwrap_or_default();
    let plan = plan_process_env(settings, &last);
    let mut next: HashMap<&'static str, String> = HashMap::new();
    for (name, action) in plan {
        match action {
            EnvAction::Set(value) => {
                std::env::set_var(name, &value);
                next.insert(name, value);
            }
            EnvAction::RemoveIfValueIs(previous) => {
                let still_ours = std::env::var(name).ok().as_deref() == Some(previous.as_str());
                if still_ours {
                    std::env::remove_var(name);
                }
            }
        }
    }
    *guard = Some(next);
}

// ---- WebView2 硬件加速 ----

/// 把 --disable-gpu 并进 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS 既有值
/// （保留 QA 用的 remote-debugging 端口等参数；browser_panel 的 sanitize
/// 只剥 remote-debugging 标志，--disable-gpu 会保留并作用于面板）。
pub(crate) fn merge_webview2_disable_gpu(existing: &str) -> String {
    let has_flag = existing.split_whitespace().any(|token| token == "--disable-gpu");
    if has_flag {
        return existing.to_string();
    }
    if existing.trim().is_empty() {
        "--disable-gpu".to_string()
    } else {
        format!("{existing} --disable-gpu")
    }
}

/// 必须在任何 WebView2 环境创建前调用（main.rs GUI 启动路径最早）。
pub fn apply_webview2_env_overrides() {
    if load().webview2_hardware_acceleration {
        return;
    }
    let existing = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS").unwrap_or_default();
    let merged = merge_webview2_disable_gpu(&existing);
    std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", merged);
}

// ---- Tauri 命令 ----

fn status_from(settings: GeneralSettings) -> GeneralSettingsStatus {
    let proxy_env_active = !settings.http_proxy.trim().is_empty();
    GeneralSettingsStatus {
        keep_awake_active: crate::keep_awake::current_state(),
        warning: ca_warning(&settings),
        proxy_env_active,
        settings,
    }
}

#[tauri::command]
pub fn general_settings_get() -> Result<GeneralSettingsStatus, String> {
    Ok(status_from((*load_cached()).clone()))
}

#[tauri::command]
pub fn general_settings_save(
    app: AppHandle,
    settings: GeneralSettings,
) -> Result<GeneralSettingsStatus, String> {
    validate_at(&settings).map_err(|error| format!("{error:#}"))?;
    let config_dir = crate::providers::config_dir().map_err(|error| format!("{error:#}"))?;
    save_settings_at(&settings_path_at(&config_dir), &settings)
        .map_err(|error| format!("{error:#}"))?;
    refresh_cache(&settings);
    apply_process_env(&settings);
    crate::keep_awake::set_enabled(settings.keep_awake);
    // 终端字体等前端消费方监听该事件即时应用。
    let _ = app.emit("general-settings-changed", &settings);
    Ok(status_from(settings))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cc-general-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn defaults_are_migration_safe() {
        let settings = GeneralSettings::default();
        assert!(settings.http_proxy.is_empty());
        assert!(settings.close_to_tray);
        assert!(settings.keep_awake);
        assert!(settings.webview2_hardware_acceleration);
        assert_eq!(settings.terminal_font_size, 12);
        // 兼容只有部分字段的旧文件。
        let partial: GeneralSettings =
            serde_json::from_str(r#"{"terminalFontSize":16}"#).unwrap();
        assert_eq!(partial.terminal_font_size, 16);
        assert!(partial.close_to_tray);
    }

    #[test]
    fn settings_roundtrip() {
        let dir = temp_dir();
        let path = settings_path_at(&dir);
        let settings = GeneralSettings {
            http_proxy: "http://127.0.0.1:7890".into(),
            http_no_proxy: ".corp.com".into(),
            http_ca_cert_path: r"C:\certs\root.pem".into(),
            close_to_tray: false,
            keep_awake: false,
            terminal_font_family: "MesloLGS NF".into(),
            terminal_font_size: 14,
            webview2_hardware_acceleration: false,
        };
        save_settings_at(&path, &settings).unwrap();
        assert_eq!(load_settings_at(&path).unwrap(), settings);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn effective_no_proxy_always_keeps_loopback() {
        assert_eq!(
            effective_no_proxy(""),
            "localhost,127.0.0.1,::1"
        );
        assert_eq!(
            effective_no_proxy(".corp.com, localhost "),
            ".corp.com,localhost,127.0.0.1,::1"
        );
        // 用户已含条目时不重复追加（大小写不敏感）。
        let lowered = effective_no_proxy("LOCALHOST,::1").to_lowercase();
        assert_eq!(lowered.matches("localhost").count(), 1);
    }

    #[test]
    fn plan_env_sets_and_rolls_back() {
        let mut settings = GeneralSettings::default();
        settings.http_proxy = "http://127.0.0.1:7890".into();
        settings.http_ca_cert_path = r"C:\certs\root.pem".into();
        let plan = plan_process_env(&settings, &HashMap::new());
        assert!(plan.contains(&(
            "HTTP_PROXY",
            EnvAction::Set("http://127.0.0.1:7890".into())
        )));
        assert!(plan.contains(&(
            "NO_PROXY",
            EnvAction::Set("localhost,127.0.0.1,::1".into())
        )));
        assert!(plan.contains(&(
            "NODE_EXTRA_CA_CERTS",
            EnvAction::Set(r"C:\certs\root.pem".into())
        )));

        // 清空配置：只回滚我们写过的值。
        let mut last = HashMap::new();
        last.insert("HTTP_PROXY", "http://127.0.0.1:7890".to_string());
        last.insert("HTTPS_PROXY", "http://127.0.0.1:7890".to_string());
        last.insert("NODE_EXTRA_CA_CERTS", r"C:\certs\root.pem".to_string());
        let cleared = GeneralSettings::default();
        let plan = plan_process_env(&cleared, &last);
        assert_eq!(plan.len(), 3);
        assert!(plan.contains(&(
            "HTTP_PROXY",
            EnvAction::RemoveIfValueIs("http://127.0.0.1:7890".into())
        )));
        // 从未写过 → 无动作（不动用户系统 env）。
        assert!(plan_process_env(&cleared, &HashMap::new()).is_empty());
    }

    #[test]
    fn webview2_merge_preserves_existing_args() {
        assert_eq!(merge_webview2_disable_gpu(""), "--disable-gpu");
        assert_eq!(
            merge_webview2_disable_gpu("--remote-debugging-port=9224"),
            "--remote-debugging-port=9224 --disable-gpu"
        );
        assert_eq!(
            merge_webview2_disable_gpu("--disable-gpu --others"),
            "--disable-gpu --others"
        );
    }

    #[test]
    fn validation_rejects_bad_values() {
        let mut settings = GeneralSettings::default();
        settings.http_proxy = "socks5://127.0.0.1:1080".into();
        assert!(validate_at(&settings).is_err());
        settings.http_proxy = "127.0.0.1:7890".into();
        assert!(validate_at(&settings).is_err());
        settings.http_proxy = String::new();
        settings.terminal_font_size = 4;
        assert!(validate_at(&settings).is_err());
        settings.terminal_font_size = 30;
        assert!(validate_at(&settings).is_err());
        settings.terminal_font_size = 12;
        settings.http_ca_cert_path = "relative/root.pem".into();
        assert!(validate_at(&settings).is_err());
        settings.http_ca_cert_path = String::new();
        settings.http_proxy = "http://proxy.internal:8080".into();
        assert!(validate_at(&settings).is_ok());
    }

    #[test]
    fn configure_http_client_builds_with_proxy() {
        let settings = GeneralSettings {
            http_proxy: "http://127.0.0.1:7890".into(),
            http_no_proxy: ".corp.com".into(),
            ..GeneralSettings::default()
        };
        refresh_cache(&settings);
        let client = configure_http_client(reqwest::Client::builder())
            .build()
            .expect("带代理的客户端应当构建成功");
        drop(client);
        refresh_cache(&GeneralSettings::default());
    }
}
