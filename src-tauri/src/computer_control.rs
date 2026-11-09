// 电脑控制（内置功能）：设置开关 = 注册一个本地 stdio MCP server
// （crewvela --computer-control-mcp，运行时移植自 orca MIT 的 PowerShell
// UIA/SendInput 实现）+ 向 claude/codex/kimi 全局部署使用技能。
// 注入管道复用浏览器控制的全部既有路径（原生聊天/多智能体/claude/codex PTY）。

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::mcp::{self, McpServerConfig, McpTransport};
use crate::skill_center::{DeploySkillInput, ImportSkillInput, SkillCenter};

pub(crate) const BUILTIN_SOURCE_KEY: &str = "builtin@computer-control";
const MCP_SERVER_NAME: &str = "computer-control";
const SETTINGS_FILE: &str = "computer-control.json";
const SKILL_DIR_NAME: &str = "computer-control-skill";
/// 技能部署的 runtime（与 skill_center 的 adapter id 对齐）。
const SKILL_RUNTIMES: [&str; 3] = ["claude_code", "codex", "kimi_code"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerControlSettings {
    #[serde(default)]
    pub enabled: bool,
}

impl Default for ComputerControlSettings {
    fn default() -> Self {
        Self { enabled: false }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputerControlStatus {
    pub settings: ComputerControlSettings,
    /// PowerShell 解析结果说明。
    pub runtime: String,
    pub warning: Option<String>,
}

// ---- 设置持久化 ----

fn settings_path_at(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE)
}

fn load_settings_at(path: &Path) -> Result<ComputerControlSettings> {
    if !path.exists() {
        return Ok(ComputerControlSettings::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("读取电脑控制配置失败: {}", path.display()))?;
    serde_json::from_str(&raw).context("解析 computer-control.json 失败")
}

fn save_settings_at(path: &Path, settings: &ComputerControlSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(settings).context("序列化电脑控制配置失败")?,
    )?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

// ---- 运行时物化与状态 ----

fn runtime_dir_at(config_dir: &Path) -> PathBuf {
    config_dir.join("computer-control")
}

fn materialize_runtime_at(config_dir: &Path) -> Result<PathBuf> {
    let path = runtime_dir_at(config_dir).join("runtime.ps1");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        &path,
        include_str!("../resources/computer-use-runtime.ps1"),
    )?;
    Ok(path)
}

fn resolve_powershell() -> Result<PathBuf> {
    which::which("powershell.exe").context("未找到 powershell.exe")
}

// ---- MCP 条目构造 ----

pub(crate) fn build_mcp_config(
    settings: &ComputerControlSettings,
    runtime_ps1: &Path,
) -> McpServerConfig {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    let exe = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("CrewVela.exe"));
    McpServerConfig {
        id: uuid::Uuid::new_v4().to_string(),
        name: MCP_SERVER_NAME.into(),
        transport: McpTransport::Stdio,
        command: Some(exe.display().to_string()),
        args: vec![
            "--computer-control-mcp".into(),
            runtime_ps1.display().to_string(),
        ],
        env: Default::default(),
        url: None,
        headers: Default::default(),
        enabled: settings.enabled,
        source_plugin: Some(BUILTIN_SOURCE_KEY.into()),
        created_at: now,
        updated_at: now,
    }
}

// ---- 技能同步 ----

fn materialize_skill_at(config_dir: &Path) -> Result<PathBuf> {
    let dir = config_dir.join(SKILL_DIR_NAME);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("SKILL.md"),
        include_str!("../../skills/computer-control/SKILL.md"),
    )?;
    Ok(dir)
}

fn undeploy_skill_everywhere(center: &SkillCenter, source_ref: &str) -> Result<()> {
    for asset_id in center.asset_ids_by_source_ref(source_ref)? {
        let deployments = center.deployment_ids_for_skill(&asset_id)?;
        if !deployments.is_empty() {
            center.undeploy_batch(deployments)?;
        }
        let _ = center.delete_asset(&asset_id);
    }
    Ok(())
}

fn retire_stale_skill_assets(center: &SkillCenter, keep_id: &str, source_ref: &str) -> Result<()> {
    for asset_id in center.asset_ids_by_source_ref(source_ref)? {
        if asset_id == keep_id {
            continue;
        }
        let deployments = center.deployment_ids_for_skill(&asset_id)?;
        if !deployments.is_empty() {
            center.undeploy_batch(deployments)?;
        }
        let _ = center.delete_asset(&asset_id);
    }
    Ok(())
}

// ---- 同步主流程 ----

pub(crate) struct SyncOutcome {
    pub runtime_label: String,
    pub warnings: Vec<String>,
}

pub(crate) fn sync_at(
    config_dir: &Path,
    mcp_file: &Path,
    center: &SkillCenter,
    settings: &ComputerControlSettings,
) -> Result<SyncOutcome> {
    let mut warnings = Vec::new();
    let skill_source_ref = config_dir
        .join(SKILL_DIR_NAME)
        .display()
        .to_string();
    if !settings.enabled {
        mcp::remove_plugin_servers_at(mcp_file, BUILTIN_SOURCE_KEY)?;
        if let Err(error) = undeploy_skill_everywhere(center, &skill_source_ref) {
            warnings.push(format!("撤下电脑控制技能失败：{error:#}"));
        }
        return Ok(SyncOutcome {
            runtime_label: "已关闭".into(),
            warnings,
        });
    }

    let powershell = resolve_powershell()?;
    let runtime_ps1 = materialize_runtime_at(config_dir)?;
    let config = build_mcp_config(settings, &runtime_ps1);
    mcp::upsert_builtin_server_at(mcp_file, BUILTIN_SOURCE_KEY, config)?;

    let source_dir = materialize_skill_at(config_dir)?;
    let source_ref = source_dir.display().to_string();
    let imported = center
        .import(ImportSkillInput {
            source_type: "local".into(),
            source: source_ref.clone(),
            branch: None,
            subpath: None,
            revision: None,
        })
        .map_err(|error| anyhow::anyhow!("导入电脑控制技能失败：{error:#}"))?;
    let asset = imported
        .first()
        .ok_or_else(|| anyhow::anyhow!("导入电脑控制技能失败：未识别到 SKILL.md"))?;
    let _ = center.set_trust(&asset.id, "trusted");
    retire_stale_skill_assets(center, &asset.id, &source_ref)?;
    let deployments = SKILL_RUNTIMES
        .iter()
        .map(|runtime| DeploySkillInput {
            skill_id: asset.id.clone(),
            runtime: (*runtime).into(),
            scope: "global".into(),
            project_path: None,
            task_id: None,
            agent_id: None,
            trust_confirmed: true,
        })
        .collect();
    if let Err(error) = center.deploy_batch(deployments) {
        warnings.push(format!("部署电脑控制技能失败：{error:#}"));
    }
    Ok(SyncOutcome {
        runtime_label: format!(
            "PowerShell 运行时已就绪（{}，UIA + SendInput，移植自 orca/MIT）",
            powershell.display()
        ),
        warnings,
    })
}

fn sync(settings: &ComputerControlSettings, center: &SkillCenter) -> Result<SyncOutcome> {
    let config_dir = crate::providers::config_dir()?;
    sync_at(
        &config_dir,
        &config_dir.join("mcp-servers.json"),
        center,
        settings,
    )
}

/// 面板读取：设置 + 运行时状态；条目漂移时以 computer-control.json 为准自愈。
fn status(app: &AppHandle) -> Result<ComputerControlStatus> {
    let config_dir = crate::providers::config_dir()?;
    let settings = load_settings_at(&settings_path_at(&config_dir))?;
    let center = app.state::<SkillCenter>();
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
            let outcome = sync(&settings, &center)?;
            return Ok(status_from(&settings, &outcome));
        }
    }
    let runtime = if settings.enabled {
        resolve_powershell()
            .map(|path| format!("PowerShell 运行时（{}）", path.display()))
            .unwrap_or_else(|_| "未找到 powershell.exe".into())
    } else {
        "已关闭".into()
    };
    Ok(ComputerControlStatus {
        settings,
        runtime,
        warning: None,
    })
}

fn status_from(settings: &ComputerControlSettings, outcome: &SyncOutcome) -> ComputerControlStatus {
    ComputerControlStatus {
        settings: settings.clone(),
        runtime: outcome.runtime_label.clone(),
        warning: outcome.warnings.first().cloned(),
    }
}

fn save(app: AppHandle, settings: ComputerControlSettings) -> Result<ComputerControlStatus> {
    let config_dir = crate::providers::config_dir()?;
    if !cfg!(target_os = "windows") {
        anyhow::bail!("电脑控制当前仅支持 Windows");
    }
    if settings.enabled {
        resolve_powershell()?;
    }
    save_settings_at(&settings_path_at(&config_dir), &settings)?;
    let center = app.state::<SkillCenter>();
    let outcome = sync(&settings, &center)?;
    Ok(status_from(&settings, &outcome))
}

// ---- Tauri 命令 ----

#[tauri::command]
pub fn computer_control_get(app: AppHandle) -> Result<ComputerControlStatus, String> {
    status(&app).map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub fn computer_control_save(
    app: AppHandle,
    settings: ComputerControlSettings,
) -> Result<ComputerControlStatus, String> {
    save(app, settings).map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cc-computer-control-{}", uuid::Uuid::new_v4()));
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
        save_settings_at(&path, &ComputerControlSettings { enabled: false }).unwrap();
        assert!(!load_settings_at(&path).unwrap().enabled);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mcp_config_points_to_exe_subcommand_with_runtime() {
        let runtime = Path::new(r"C:\conf\computer-control\runtime.ps1");
        let config = build_mcp_config(&ComputerControlSettings { enabled: true }, runtime);
        assert_eq!(config.name, "computer-control");
        assert_eq!(config.transport, McpTransport::Stdio);
        assert_eq!(config.source_plugin.as_deref(), Some(BUILTIN_SOURCE_KEY));
        assert!(config.enabled);
        assert_eq!(
            config.args,
            vec![
                "--computer-control-mcp".to_string(),
                r"C:\conf\computer-control\runtime.ps1".to_string(),
            ]
        );
        assert!(config.command.as_deref().is_some_and(|exe| !exe.is_empty()));
    }

    #[test]
    fn materialize_runtime_writes_orca_script() {
        let dir = temp_dir();
        let runtime = materialize_runtime_at(&dir).unwrap();
        let content = std::fs::read_to_string(&runtime).unwrap();
        assert!(content.contains("Invoke-OrcaServeLoop"));
        assert!(content.contains("stablyai/orca"));
        assert!(content.contains("MIT License"));
        assert_eq!(
            runtime,
            runtime_dir_at(&dir).join("runtime.ps1")
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sync_disable_removes_entry_and_skill_deployments() {
        let dir = temp_dir();
        let mcp_file = dir.join("mcp-servers.json");
        let center = SkillCenter::open_at(dir.join("skill-center")).unwrap();

        // 预置：启用态条目 + 一个已部署技能（project 作用域指向沙箱）。
        let runtime = materialize_runtime_at(&dir).unwrap();
        let entry = build_mcp_config(&ComputerControlSettings { enabled: true }, &runtime);
        mcp::upsert_builtin_server_at(&mcp_file, BUILTIN_SOURCE_KEY, entry).unwrap();
        let source = materialize_skill_at(&dir).unwrap();
        let imported = center
            .import(ImportSkillInput {
                source_type: "local".into(),
                source: source.display().to_string(),
                branch: None,
                subpath: None,
                revision: None,
            })
            .unwrap();
        let asset_id = imported[0].id.clone();
        let project = dir.join("project");
        std::fs::create_dir_all(&project).unwrap();
        center
            .deploy_batch(vec![DeploySkillInput {
                skill_id: asset_id.clone(),
                runtime: "claude_code".into(),
                scope: "project".into(),
                project_path: Some(project.display().to_string()),
                task_id: None,
                agent_id: None,
                trust_confirmed: true,
            }])
            .unwrap();
        assert_eq!(mcp::load_at(&mcp_file).unwrap().len(), 1);
        assert!(!center.deployment_ids_for_skill(&asset_id).unwrap().is_empty());

        let outcome = sync_at(
            &dir,
            &mcp_file,
            &center,
            &ComputerControlSettings { enabled: false },
        )
        .unwrap();
        assert!(outcome.warnings.is_empty());
        assert!(mcp::load_at(&mcp_file).unwrap().is_empty());
        assert!(center
            .asset_ids_by_source_ref(&source.display().to_string())
            .unwrap()
            .is_empty());
        assert!(center.deployment_ids_for_skill(&asset_id).unwrap().is_empty());
        drop(center);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
