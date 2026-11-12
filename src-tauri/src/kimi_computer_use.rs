// kimi-computer-use（内置功能）：第二种电脑控制方式，与内置
// computer_control（PowerShell UIA/SendInput）相互独立。开启后仅注册
// KimiCU 官方 kimi-cu.exe 为全局 stdio MCP server（参数 mcp），
// 不物化运行时、不部署技能。exe 固定取 %LOCALAPPDATA%\KimiCU\kimi-cu.exe。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::mcp::{self, McpServerConfig, McpTransport};

pub(crate) const BUILTIN_SOURCE_KEY: &str = "builtin@kimi-computer-use";
const MCP_SERVER_NAME: &str = "kimi-cu";
const SETTINGS_FILE: &str = "kimi-computer-use.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiComputerUseSettings {
    #[serde(default)]
    pub enabled: bool,
}

impl Default for KimiComputerUseSettings {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KimiComputerUseStatus {
    pub settings: KimiComputerUseSettings,
    pub installed: bool,
    pub mcp_registered: Option<bool>,
    /// KimiCU 可执行文件解析结果说明。
    pub runtime: String,
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub macos: Option<crate::macos_computer_use::MacControlStatus>,
}

// ---- 设置持久化 ----

fn settings_path_at(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE)
}

fn load_settings_at(path: &Path) -> Result<KimiComputerUseSettings> {
    if !path.exists() {
        return Ok(KimiComputerUseSettings::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("读取 kimi-computer-use 配置失败: {}", path.display()))?;
    serde_json::from_str(&raw).context("解析 kimi-computer-use.json 失败")
}

fn save_settings_at(path: &Path, settings: &KimiComputerUseSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(settings).context("序列化 kimi-computer-use 配置失败")?,
    )?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

// ---- 可执行文件定位 ----

/// KimiCU 默认安装位置：%LOCALAPPDATA%\KimiCU\kimi-cu.exe。
fn exe_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(|base| PathBuf::from(base).join("KimiCU").join("kimi-cu.exe"))
        .unwrap_or_else(|| PathBuf::from(r"%LOCALAPPDATA%\KimiCU\kimi-cu.exe"))
}

fn ensure_exe_at(exe: &Path) -> Result<()> {
    if !exe.exists() {
        anyhow::bail!(
            "未找到 kimi-cu.exe（{}），请先安装 KimiCU 后再开启",
            exe.display()
        );
    }
    Ok(())
}

// ---- MCP 条目构造 ----

pub(crate) fn build_mcp_config(
    settings: &KimiComputerUseSettings,
    exe: &Path,
) -> McpServerConfig {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    McpServerConfig {
        id: uuid::Uuid::new_v4().to_string(),
        name: MCP_SERVER_NAME.into(),
        transport: McpTransport::Stdio,
        command: Some(exe.display().to_string()),
        args: vec!["mcp".into()],
        env: Default::default(),
        url: None,
        headers: Default::default(),
        enabled: settings.enabled,
        source_plugin: Some(BUILTIN_SOURCE_KEY.into()),
        created_at: now,
        updated_at: now,
    }
}

// ---- 同步主流程 ----

#[derive(Debug)]
pub(crate) struct SyncOutcome {
    pub runtime_label: String,
    pub warnings: Vec<String>,
}

pub(crate) fn sync_at(
    mcp_file: &Path,
    settings: &KimiComputerUseSettings,
    exe: &Path,
) -> Result<SyncOutcome> {
    if !settings.enabled {
        mcp::remove_plugin_servers_at(mcp_file, BUILTIN_SOURCE_KEY)?;
        return Ok(SyncOutcome {
            runtime_label: "已关闭".into(),
            warnings: Vec::new(),
        });
    }
    ensure_exe_at(exe)?;
    let config = build_mcp_config(settings, exe);
    mcp::upsert_builtin_server_at(mcp_file, BUILTIN_SOURCE_KEY, config)?;
    Ok(SyncOutcome {
        runtime_label: format!("KimiCU 运行时已就绪（{}，stdio MCP）", exe.display()),
        warnings: Vec::new(),
    })
}

fn sync(settings: &KimiComputerUseSettings) -> Result<SyncOutcome> {
    let config_dir = crate::providers::config_dir()?;
    sync_at(
        &config_dir.join("mcp-servers.json"),
        settings,
        &exe_path(),
    )
}

/// 面板读取：设置 + 运行时状态；条目漂移时以 kimi-computer-use.json 为准自愈。
fn status(app: &AppHandle) -> Result<KimiComputerUseStatus> {
    #[cfg(target_os = "macos")]
    { let _ = app; return mac_status(); }
    let _ = app;
    let config_dir = crate::providers::config_dir()?;
    status_at(&config_dir, &exe_path())
}

fn status_at(config_dir: &Path, exe: &Path) -> Result<KimiComputerUseStatus> {
    let settings = load_settings_at(&settings_path_at(config_dir))?;
    let mcp_file = config_dir.join("mcp-servers.json");
    let servers = mcp::load_at(&mcp_file);
    let mcp_registered = servers.as_ref().ok().map(|entries| {
        entries.iter().any(|entry| entry.enabled && entry.source_plugin.as_deref() == Some(BUILTIN_SOURCE_KEY))
    });
    if settings.enabled && mcp_registered == Some(false) {
        let outcome = sync_at(&mcp_file, &settings, exe)?;
        return Ok(status_from(&settings, &outcome, exe));
    }
    let runtime = if settings.enabled {
        if exe.exists() {
            format!("KimiCU 运行时（{}）", exe.display())
        } else {
            format!("未找到 kimi-cu.exe（{}），请先安装 KimiCU", exe.display())
        }
    } else {
        "已关闭".into()
    };
    Ok(KimiComputerUseStatus {
        settings,
        installed: exe.is_file(),
        mcp_registered,
        runtime,
        warning: servers.err().map(|error| format!("读取 MCP 注册状态失败：{error}")),
        macos: None,
    })
}

fn status_from(settings: &KimiComputerUseSettings, outcome: &SyncOutcome, exe: &Path) -> KimiComputerUseStatus {
    KimiComputerUseStatus {
        settings: settings.clone(),
        installed: exe.is_file(),
        mcp_registered: Some(settings.enabled),
        runtime: outcome.runtime_label.clone(),
        warning: outcome.warnings.first().cloned(),
        macos: None,
    }
}

fn save(settings: KimiComputerUseSettings) -> Result<KimiComputerUseStatus> {
    #[cfg(target_os = "macos")]
    {
        let dir = crate::providers::config_dir()?;
        crate::macos_computer_use::save(crate::macos_computer_use::MacControlTool::KimiCu, &dir, &settings_path_at(&dir), settings.enabled)?;
        return mac_status();
    }
    if !cfg!(target_os = "windows") {
        anyhow::bail!("kimi-computer-use 当前仅支持 Windows");
    }
    if settings.enabled {
        ensure_exe_at(&exe_path())?;
    }
    let config_dir = crate::providers::config_dir()?;
    save_settings_at(&settings_path_at(&config_dir), &settings)?;
    let outcome = sync(&settings)?;
    Ok(status_from(&settings, &outcome, &exe_path()))
}


#[cfg(target_os = "macos")]
fn mac_status() -> Result<KimiComputerUseStatus> {
    let dir = crate::providers::config_dir()?;
    let settings = load_settings_at(&settings_path_at(&dir))?;
    let macos = crate::macos_computer_use::inspect(crate::macos_computer_use::MacControlTool::KimiCu, &dir, settings.enabled);
    Ok(KimiComputerUseStatus { installed: macos.installed, mcp_registered: macos.mcp_registered,
        runtime: macos.executable.clone().map(|p| format!("本机执行入口：{p}（stdio MCP）")).unwrap_or_else(|| "未检测到可用运行时".into()),
        warning: Some(macos.detail.clone()), settings, macos: Some(macos),
    })
}

// ---- Tauri 命令 ----

#[tauri::command]
pub async fn kimi_computer_use_get(app: AppHandle) -> Result<KimiComputerUseStatus, String> {
    tauri::async_runtime::spawn_blocking(move || status(&app)).await.map_err(|e| e.to_string())?.map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub async fn kimi_computer_use_save(
    settings: KimiComputerUseSettings,
) -> Result<KimiComputerUseStatus, String> {
    tauri::async_runtime::spawn_blocking(move || save(settings)).await.map_err(|e| e.to_string())?.map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("kimi-computer-use-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn status_distinguishes_installation_registration_and_unreadable_registry() {
        let dir = temp_dir();
        let exe = dir.join("kimi-cu.exe");
        let missing = status_at(&dir, &exe).unwrap();
        assert!(!missing.installed);
        assert_eq!(missing.mcp_registered, Some(false));
        std::fs::write(&exe, b"").unwrap();
        let installed = status_at(&dir, &exe).unwrap();
        assert!(installed.installed);
        assert_eq!(installed.mcp_registered, Some(false));
        save_settings_at(&settings_path_at(&dir), &KimiComputerUseSettings { enabled: true }).unwrap();
        assert_eq!(status_at(&dir, &exe).unwrap().mcp_registered, Some(true));
        std::fs::write(dir.join("mcp-servers.json"), b"broken").unwrap();
        let unknown = status_at(&dir, &exe).unwrap();
        assert_eq!(unknown.mcp_registered, None);
        assert!(unknown.warning.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn settings_roundtrip_and_default_off() {
        let dir = temp_dir();
        let path = settings_path_at(&dir);
        assert!(!load_settings_at(&path).unwrap().enabled);
        std::fs::write(&path, r#"{"enabled":true}"#).unwrap();
        assert!(load_settings_at(&path).unwrap().enabled);
        save_settings_at(&path, &KimiComputerUseSettings { enabled: false }).unwrap();
        assert!(!load_settings_at(&path).unwrap().enabled);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mcp_config_points_to_kimi_cu_exe_with_mcp_arg() {
        let exe = Path::new(r"C:\Users\demo\AppData\Local\KimiCU\kimi-cu.exe");
        let config = build_mcp_config(&KimiComputerUseSettings { enabled: true }, exe);
        assert_eq!(config.name, "kimi-cu");
        assert_eq!(config.transport, McpTransport::Stdio);
        assert_eq!(config.source_plugin.as_deref(), Some(BUILTIN_SOURCE_KEY));
        assert!(config.enabled);
        assert_eq!(config.args, vec!["mcp".to_string()]);
        assert_eq!(
            config.command.as_deref(),
            Some(r"C:\Users\demo\AppData\Local\KimiCU\kimi-cu.exe")
        );
    }

    #[test]
    fn sync_enable_registers_stdio_server() {
        let dir = temp_dir();
        let mcp_file = dir.join("mcp-servers.json");
        let exe = dir.join("kimi-cu.exe");
        std::fs::write(&exe, b"").unwrap();

        let outcome = sync_at(&mcp_file, &KimiComputerUseSettings { enabled: true }, &exe).unwrap();
        assert!(outcome.runtime_label.contains("已就绪"));
        let servers = mcp::load_at(&mcp_file).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "kimi-cu");
        assert_eq!(servers[0].args, vec!["mcp".to_string()]);
        assert_eq!(
            servers[0].command.as_deref(),
            Some(exe.display().to_string().as_str())
        );
        assert_eq!(servers[0].source_plugin.as_deref(), Some(BUILTIN_SOURCE_KEY));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sync_disable_removes_entry() {
        let dir = temp_dir();
        let mcp_file = dir.join("mcp-servers.json");
        let exe = dir.join("kimi-cu.exe");
        std::fs::write(&exe, b"").unwrap();
        sync_at(&mcp_file, &KimiComputerUseSettings { enabled: true }, &exe).unwrap();
        assert_eq!(mcp::load_at(&mcp_file).unwrap().len(), 1);

        let outcome = sync_at(&mcp_file, &KimiComputerUseSettings { enabled: false }, &exe).unwrap();
        assert_eq!(outcome.runtime_label, "已关闭");
        assert!(mcp::load_at(&mcp_file).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sync_enable_requires_existing_exe() {
        let dir = temp_dir();
        let mcp_file = dir.join("mcp-servers.json");
        let exe = dir.join("missing-kimi-cu.exe");
        let error = sync_at(&mcp_file, &KimiComputerUseSettings { enabled: true }, &exe)
            .unwrap_err()
            .to_string();
        assert!(error.contains("kimi-cu.exe"), "错误信息应指出缺失文件: {error}");
        assert!(mcp::load_at(&mcp_file).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
