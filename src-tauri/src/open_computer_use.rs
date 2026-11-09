// open-computer-use（内置功能）：第三种电脑控制方式，基于开源项目
// iFurySt/open-codex-computer-use（npm 包 open-computer-use，MIT）。
// 与内置 computer_control（PowerShell UIA）、kimi-computer-use（KimiCU）
// 相互独立。开启后仅注册其原生 exe 为全局 stdio MCP server（参数 mcp），
// 不部署技能。exe 解析：npm 全局 node_modules 下的
// open-computer-use/dist/windows/<arch>/open-computer-use.exe。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::mcp::{self, McpServerConfig, McpTransport};

pub(crate) const BUILTIN_SOURCE_KEY: &str = "builtin@open-computer-use";
const MCP_SERVER_NAME: &str = "open-computer-use";
const SETTINGS_FILE: &str = "open-computer-use.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenComputerUseSettings {
    #[serde(default)]
    pub enabled: bool,
}

impl Default for OpenComputerUseSettings {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenComputerUseStatus {
    pub settings: OpenComputerUseSettings,
    /// open-computer-use.exe 解析结果说明。
    pub runtime: String,
    pub warning: Option<String>,
}

// ---- 设置持久化 ----

fn settings_path_at(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE)
}

fn load_settings_at(path: &Path) -> Result<OpenComputerUseSettings> {
    if !path.exists() {
        return Ok(OpenComputerUseSettings::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("读取 open-computer-use 配置失败: {}", path.display()))?;
    serde_json::from_str(&raw).context("解析 open-computer-use.json 失败")
}

fn save_settings_at(path: &Path, settings: &OpenComputerUseSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(settings).context("序列化 open-computer-use 配置失败")?,
    )?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

// ---- 可执行文件定位 ----

fn arch_dir() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        _ => "amd64",
    }
}

fn exe_at_npm_root(npm_root: &Path) -> PathBuf {
    npm_root
        .join("open-computer-use")
        .join("dist")
        .join("windows")
        .join(arch_dir())
        .join("open-computer-use.exe")
}

/// npm 安装器默认全局目录下的 exe（%APPDATA%\npm\node_modules）。
fn default_npm_root_exe() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(|appdata| {
            exe_at_npm_root(&PathBuf::from(appdata).join("npm").join("node_modules"))
        })
        .unwrap_or_else(|| PathBuf::from("open-computer-use.exe"))
}

/// 解析 npm 全局安装的原生 exe：首选 npm 安装器默认位置，未命中时通过
/// `npm root -g` 兜底（nvm/volta 等自定义全局目录），仍未命中则返回默认
/// 路径供错误提示展示。
fn exe_path() -> PathBuf {
    let default_candidate = default_npm_root_exe();
    if default_candidate.exists() {
        return default_candidate;
    }
    if let Ok(output) = std::process::Command::new("cmd")
        .args(["/C", "npm", "root", "-g"])
        .output()
    {
        if output.status.success() {
            let root = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !root.is_empty() {
                let candidate = exe_at_npm_root(&PathBuf::from(&root));
                if candidate.exists() {
                    return candidate;
                }
            }
        }
    }
    default_candidate
}

fn ensure_exe_at(exe: &Path) -> Result<()> {
    if !exe.exists() {
        anyhow::bail!(
            "未找到 open-computer-use.exe（{}），请先执行 npm i -g open-computer-use 安装后再开启",
            exe.display()
        );
    }
    Ok(())
}

// ---- MCP 条目构造 ----

pub(crate) fn build_mcp_config(
    settings: &OpenComputerUseSettings,
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
    settings: &OpenComputerUseSettings,
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
        runtime_label: format!("Open Computer Use 运行时已就绪（{}，stdio MCP）", exe.display()),
        warnings: Vec::new(),
    })
}

fn sync(settings: &OpenComputerUseSettings) -> Result<SyncOutcome> {
    let config_dir = crate::providers::config_dir()?;
    sync_at(
        &config_dir.join("mcp-servers.json"),
        settings,
        &exe_path(),
    )
}

/// 面板读取：设置 + 运行时状态；条目漂移时以 open-computer-use.json 为准自愈。
fn status(app: &AppHandle) -> Result<OpenComputerUseStatus> {
    let _ = app;
    let config_dir = crate::providers::config_dir()?;
    let settings = load_settings_at(&settings_path_at(&config_dir))?;
    if settings.enabled {
        let servers = mcp::load().unwrap_or_default();
        let drifted = match servers
            .iter()
            .find(|server| server.source_plugin.as_deref() == Some(BUILTIN_SOURCE_KEY))
        {
            Some(entry) => !entry.enabled,
            None => true,
        };
        if drifted {
            let outcome = sync(&settings)?;
            return Ok(status_from(&settings, &outcome));
        }
    }
    let exe = exe_path();
    let runtime = if settings.enabled {
        if exe.exists() {
            format!("Open Computer Use 运行时（{}）", exe.display())
        } else {
            format!(
                "未找到 open-computer-use.exe（{}），请先执行 npm i -g open-computer-use 安装",
                exe.display()
            )
        }
    } else {
        "已关闭".into()
    };
    Ok(OpenComputerUseStatus {
        settings,
        runtime,
        warning: None,
    })
}

fn status_from(settings: &OpenComputerUseSettings, outcome: &SyncOutcome) -> OpenComputerUseStatus {
    OpenComputerUseStatus {
        settings: settings.clone(),
        runtime: outcome.runtime_label.clone(),
        warning: outcome.warnings.first().cloned(),
    }
}

fn save(settings: OpenComputerUseSettings) -> Result<OpenComputerUseStatus> {
    if !cfg!(target_os = "windows") {
        anyhow::bail!("open-computer-use 当前仅支持 Windows");
    }
    if settings.enabled {
        ensure_exe_at(&exe_path())?;
    }
    let config_dir = crate::providers::config_dir()?;
    save_settings_at(&settings_path_at(&config_dir), &settings)?;
    let outcome = sync(&settings)?;
    Ok(status_from(&settings, &outcome))
}

// ---- Tauri 命令 ----

#[tauri::command]
pub fn open_computer_use_get(app: AppHandle) -> Result<OpenComputerUseStatus, String> {
    status(&app).map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub fn open_computer_use_save(
    settings: OpenComputerUseSettings,
) -> Result<OpenComputerUseStatus, String> {
    save(settings).map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("open-computer-use-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn settings_roundtrip_and_default_off() {
        let dir = temp_dir();
        let path = settings_path_at(&dir);
        assert!(!load_settings_at(&path).unwrap().enabled);
        std::fs::write(&path, r#"{"enabled":true}"#).unwrap();
        assert!(load_settings_at(&path).unwrap().enabled);
        save_settings_at(&path, &OpenComputerUseSettings { enabled: false }).unwrap();
        assert!(!load_settings_at(&path).unwrap().enabled);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mcp_config_points_to_native_exe_with_mcp_arg() {
        let exe = Path::new(r"C:\npm\node_modules\open-computer-use\dist\windows\amd64\open-computer-use.exe");
        let config = build_mcp_config(&OpenComputerUseSettings { enabled: true }, exe);
        assert_eq!(config.name, "open-computer-use");
        assert_eq!(config.transport, McpTransport::Stdio);
        assert_eq!(config.source_plugin.as_deref(), Some(BUILTIN_SOURCE_KEY));
        assert!(config.enabled);
        assert_eq!(config.args, vec!["mcp".to_string()]);
        assert_eq!(
            config.command.as_deref(),
            Some(r"C:\npm\node_modules\open-computer-use\dist\windows\amd64\open-computer-use.exe")
        );
    }

    #[test]
    fn exe_at_npm_root_uses_current_arch() {
        let path = exe_at_npm_root(Path::new(r"C:\npm\node_modules"));
        let expected_arch = if cfg!(target_arch = "aarch64") { "arm64" } else { "amd64" };
        assert_eq!(
            path,
            PathBuf::from(format!(
                r"C:\npm\node_modules\open-computer-use\dist\windows\{expected_arch}\open-computer-use.exe"
            ))
        );
    }

    #[test]
    fn sync_enable_registers_stdio_server() {
        let dir = temp_dir();
        let mcp_file = dir.join("mcp-servers.json");
        let exe = dir.join("open-computer-use.exe");
        std::fs::write(&exe, b"").unwrap();

        let outcome =
            sync_at(&mcp_file, &OpenComputerUseSettings { enabled: true }, &exe).unwrap();
        assert!(outcome.runtime_label.contains("已就绪"));
        let servers = mcp::load_at(&mcp_file).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].name, "open-computer-use");
        assert_eq!(servers[0].args, vec!["mcp".to_string()]);
        assert_eq!(
            servers[0].command.as_deref(),
            Some(exe.display().to_string().as_str())
        );
        assert_eq!(
            servers[0].source_plugin.as_deref(),
            Some(BUILTIN_SOURCE_KEY)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sync_disable_removes_entry() {
        let dir = temp_dir();
        let mcp_file = dir.join("mcp-servers.json");
        let exe = dir.join("open-computer-use.exe");
        std::fs::write(&exe, b"").unwrap();
        sync_at(&mcp_file, &OpenComputerUseSettings { enabled: true }, &exe).unwrap();
        assert_eq!(mcp::load_at(&mcp_file).unwrap().len(), 1);

        let outcome =
            sync_at(&mcp_file, &OpenComputerUseSettings { enabled: false }, &exe).unwrap();
        assert_eq!(outcome.runtime_label, "已关闭");
        assert!(mcp::load_at(&mcp_file).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sync_enable_requires_existing_exe() {
        let dir = temp_dir();
        let mcp_file = dir.join("mcp-servers.json");
        let exe = dir.join("missing-open-computer-use.exe");
        let error = sync_at(&mcp_file, &OpenComputerUseSettings { enabled: true }, &exe)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("open-computer-use.exe"),
            "错误信息应指出缺失文件: {error}"
        );
        assert!(mcp::load_at(&mcp_file).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
