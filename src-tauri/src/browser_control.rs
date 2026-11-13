// 浏览器控制（内置功能）：设置开关 = 注册一个本地 stdio MCP server（官方
// @playwright/mcp，驱动系统 Chrome/Edge）+ 向 claude/codex/kimi 全局部署使用技能。
// MCP 条目与技能随开关联动，source_plugin "builtin@browser-control" 是反向清理锚点；
// 注入管道复用既有路径：claude PTY（enabled_claude_map）、多智能体 ACP
// （enabled_acp_servers）、原生聊天 ACP（enabled_native_chat_servers）、codex PTY（-c 覆盖）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::mcp::{self, McpServerConfig, McpTransport};
use crate::skill_center::{DeploySkillInput, ImportSkillInput, SkillCenter};

pub(crate) const BUILTIN_SOURCE_KEY: &str = "builtin@browser-control";
const MCP_SERVER_NAME: &str = "browser-control";
/// 与前端 package.json 的依赖版本保持一致；release 无本地 node_modules 时 npx 兜底拉同一版本。
pub(crate) const PLAYWRIGHT_MCP_VERSION: &str = "0.0.83";
const PLAYWRIGHT_MCP_PACKAGE: &str = "@playwright/mcp";
const SETTINGS_FILE: &str = "browser-control.json";
const SKILL_DIR_NAME: &str = "browser-control-skill";
/// 技能部署的 runtime（与 skill_center 的 adapter id 对齐）。
const SKILL_RUNTIMES: [&str; 3] = ["claude_code", "codex", "kimi_code"];

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserChannel {
    #[default]
    Auto,
    Chrome,
    Msedge,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserControlSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub browser: BrowserChannel,
    /// BrowserChannel::Custom 时的浏览器可执行文件绝对路径。
    #[serde(default)]
    pub custom_path: String,
    #[serde(default)]
    pub headless: bool,
    /// 优先接管内置浏览器面板（Windows：面板子 webview 的 CDP 通道）。
    #[serde(default = "default_true")]
    pub prefer_panel: bool,
}

impl Default for BrowserControlSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            browser: BrowserChannel::Auto,
            custom_path: String::new(),
            headless: false,
            prefer_panel: true,
        }
    }
}

/// 面板状态行：设置页与桥接脚本都关心面板 CDP 通道是否活着。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserControlStatus {
    pub settings: BrowserControlSettings,
    /// 解析出的浏览器说明（如 "Chrome（C:\…\chrome.exe）"）。
    pub resolved_browser: Option<String>,
    /// 引擎（@playwright/mcp）启动方式说明。
    pub engine: String,
    /// 内置面板接管状态（None = 未启用面板优先 / 平台不支持）。
    pub panel: Option<String>,
    pub warning: Option<String>,
}

// ---- 设置持久化 ----

fn settings_path_at(config_dir: &Path) -> PathBuf {
    config_dir.join(SETTINGS_FILE)
}

fn load_settings_at(path: &Path) -> Result<BrowserControlSettings> {
    if !path.exists() {
        return Ok(BrowserControlSettings::default());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("读取浏览器控制配置失败: {}", path.display()))?;
    serde_json::from_str(&raw).context("解析 browser-control.json 失败")
}

fn save_settings_at(path: &Path, settings: &BrowserControlSettings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(
        &temp,
        serde_json::to_vec_pretty(settings).context("序列化浏览器控制配置失败")?,
    )?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

// ---- 浏览器探测 ----

fn first_existing(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.into_iter().find(|path| path.is_file()).cloned()
}

fn chrome_candidates() -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        let mut roots: Vec<PathBuf> = ["PROGRAMFILES", "ProgramFiles(x86)", "LOCALAPPDATA"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        roots.dedup();
        roots
            .into_iter()
            .map(|root| root.join(r"Google\Chrome\Application\chrome.exe"))
            .collect()
    } else if cfg!(target_os = "macos") {
        vec!["/Applications/Google Chrome.app/Contents/MacOS/Google Chrome".into()]
    } else {
        vec![
            "/usr/bin/google-chrome".into(),
            "/usr/bin/google-chrome-stable".into(),
        ]
    }
}

fn edge_candidates() -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        let mut roots: Vec<PathBuf> = ["ProgramFiles(x86)", "PROGRAMFILES"]
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .collect();
        roots.dedup();
        roots
            .into_iter()
            .map(|root| root.join(r"Microsoft\Edge\Application\msedge.exe"))
            .collect()
    } else if cfg!(target_os = "macos") {
        vec!["/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge".into()]
    } else {
        vec!["/usr/bin/microsoft-edge".into()]
    }
}

/// 解析结果：传给 @playwright/mcp 的 --browser channel 或 --executable-path。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedBrowser {
    /// 传给引擎的参数（已含 "--browser <channel>" 或 "--executable-path <path>"）。
    pub engine_args: Vec<String>,
    /// 面板展示用说明。
    pub label: String,
}

pub(crate) fn resolve_browser(settings: &BrowserControlSettings) -> Result<ResolvedBrowser> {
    let missing_hint =
        "未找到可用的 Chrome 或 Edge，请安装其中之一后重试，或在浏览器控制设置里选择自定义路径";
    match settings.browser {
        BrowserChannel::Custom => {
            let path = PathBuf::from(settings.custom_path.trim());
            if settings.custom_path.trim().is_empty() || !path.is_file() {
                bail!("自定义浏览器路径无效或文件不存在：{}", settings.custom_path);
            }
            Ok(ResolvedBrowser {
                engine_args: vec![
                    "--executable-path".into(),
                    path.display().to_string(),
                ],
                label: format!("自定义（{}）", path.display()),
            })
        }
        BrowserChannel::Chrome => {
            let executable = first_existing(&chrome_candidates())
                .ok_or_else(|| anyhow!("未检测到已安装的 Chrome。{missing_hint}"))?;
            Ok(ResolvedBrowser {
                engine_args: vec!["--browser".into(), "chrome".into()],
                label: format!("Chrome（{}）", executable.display()),
            })
        }
        BrowserChannel::Msedge => {
            let executable = first_existing(&edge_candidates())
                .ok_or_else(|| anyhow!("未检测到已安装的 Edge。{missing_hint}"))?;
            Ok(ResolvedBrowser {
                engine_args: vec!["--browser".into(), "msedge".into()],
                label: format!("Edge（{}）", executable.display()),
            })
        }
        BrowserChannel::Auto => {
            if let Some(executable) = first_existing(&chrome_candidates()) {
                return Ok(ResolvedBrowser {
                    engine_args: vec!["--browser".into(), "chrome".into()],
                    label: format!("Chrome（{}）", executable.display()),
                });
            }
            if let Some(executable) = first_existing(&edge_candidates()) {
                return Ok(ResolvedBrowser {
                    engine_args: vec!["--browser".into(), "msedge".into()],
                    label: format!("Edge（{}）", executable.display()),
                });
            }
            bail!("自动探测未找到 Chrome 或 Edge。{missing_hint}");
        }
    }
}

// ---- 引擎（@playwright/mcp）启动方式解析 ----

/// 引擎启动前缀：MCP 条目的 command/args 以它开头，后接浏览器参数。
/// 优先 `node <cli.js>` 形态——直接 spawn 可执行文件对各 Agent 的 stdio MCP
/// 客户端最稳（.cmd/.bat 在部分客户端需要 shell，会静默失败）。
/// `node_path` 供面板优先模式下的桥接脚本作入口解释器。
pub(crate) struct EngineLaunch {
    pub command: String,
    pub args: Vec<String>,
    pub node_path: String,
    /// 面板展示用说明。
    pub label: String,
    pub note: Option<String>,
}

pub(crate) fn resolve_engine() -> Result<EngineLaunch> {
    let node = which::which("node").context("未找到 Node.js；浏览器控制需要 Node.js 22+")?;
    let node_path = node.display().to_string();
    // 发行形态只查安装位置和运行时 PATH，不带入构建机器的源码目录。
    for root in crate::runtime_dependencies::node_modules_candidates() {
      let local = root.join(PLAYWRIGHT_MCP_PACKAGE).join("cli.js");
      if local.is_file() {
        return Ok(EngineLaunch {
            command: node_path.clone(),
            args: vec![local.display().to_string()],
            node_path,
            label: format!("已安装依赖 @playwright/mcp（{}）", local.display()),
            note: None,
        });
      }
    }
    // 2) 全局 npm 安装。
    for global_root in global_node_modules_candidates() {
        let cli = global_root.join(PLAYWRIGHT_MCP_PACKAGE).join("cli.js");
        if cli.is_file() {
            return Ok(EngineLaunch {
                command: node_path.clone(),
                args: vec![cli.display().to_string()],
                node_path,
                label: format!("全局安装 @playwright/mcp（{}）", cli.display()),
                note: None,
            });
        }
    }
    // 3) npx 兜底（首次联网下载；.cmd 形态可能不被部分 Agent 的 stdio 客户端拉起）。
    let npx = which::which(if cfg!(target_os = "windows") {
        "npx.cmd"
    } else {
        "npx"
    })
    .context("未找到 npx；请安装 Node.js 22+ 或手动 npm i -g @playwright/mcp")?;
    Ok(EngineLaunch {
        command: npx.display().to_string(),
        args: vec![
            "--yes".into(),
            format!("{PLAYWRIGHT_MCP_PACKAGE}@{PLAYWRIGHT_MCP_VERSION}"),
        ],
        node_path,
        label: format!("npx 兜底 {PLAYWRIGHT_MCP_PACKAGE}@{PLAYWRIGHT_MCP_VERSION}"),
        note: Some(
            "未找到本地/全局安装的 @playwright/mcp，将用 npx 拉取（首次需要联网）；\
             建议执行 npm i -g @playwright/mcp 以获得最稳的启动方式"
                .into(),
        ),
    })
}

fn resolve_control_browser(settings: &BrowserControlSettings) -> Result<ResolvedBrowser> {
    if cfg!(target_os = "macos") && settings.prefer_panel {
        Ok(ResolvedBrowser { engine_args: Vec::new(), label: "内置 WKWebView".into() })
    } else { resolve_browser(settings) }
}

fn resolve_control_engine(settings: &BrowserControlSettings) -> Result<EngineLaunch> {
    if cfg!(target_os = "macos") && settings.prefer_panel {
        let node = which::which("node").context("未找到 Node.js；面板 MCP 接入需要 Node.js 22+")?.display().to_string();
        Ok(EngineLaunch { command: node.clone(), node_path: node, args: Vec::new(), label: "内置 WKWebView MCP 适配器".into(), note: None })
    } else { resolve_engine() }
}

fn global_node_modules_candidates() -> Vec<PathBuf> {
    if cfg!(target_os = "windows") {
        std::env::var_os("APPDATA")
            .map(|appdata| PathBuf::from(appdata).join("npm").join("node_modules"))
            .into_iter()
            .collect()
    } else {
        vec![
            PathBuf::from("/usr/local/lib/node_modules"),
            PathBuf::from("/usr/lib/node_modules"),
        ]
    }
}

// ---- MCP 条目构造 ----

/// 面板优先模式的桥接脚本路径（与端口文件同目录）。
fn bridge_path_at(config_dir: &Path) -> PathBuf {
    config_dir.join("browser-control").join("bridge.mjs")
}

fn materialize_bridge_at(config_dir: &Path) -> Result<PathBuf> {
    let path = bridge_path_at(config_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        &path,
        bridge_source(),
    )?;
    #[cfg(target_os = "macos")]
    std::fs::write(path.with_file_name("browser-panel-dom.mjs"), include_str!("../resources/browser-panel-dom.mjs"))?;
    Ok(path)
}

fn bridge_source() -> &'static str {
    if cfg!(target_os = "macos") { include_str!("../resources/browser-control-macos.mjs") }
    else { include_str!("../resources/browser-control-bridge.mjs") }
}

/// 桥接脚本升级保活：物化文件与程序内置版本不一致时重写（例如安装新版本后
/// 旧 bridge 缺少按工具调用激活等修复）。此操作不创建或显示浏览器。
fn refresh_bridge_materialization(config_dir: &Path) {
    let builtin = bridge_source();
    let path = bridge_path_at(config_dir);
    match std::fs::read_to_string(&path) {
        Ok(current) if current == builtin && (!cfg!(target_os = "macos") ||
            std::fs::read_to_string(path.with_file_name("browser-panel-dom.mjs")).ok().as_deref() == Some(include_str!("../resources/browser-panel-dom.mjs"))) => {}
        _ => {
            if let Err(error) = materialize_bridge_at(config_dir) {
                eprintln!("[crewvela] 刷新浏览器控制桥接脚本失败：{error:#}");
            }
        }
    }
}

pub fn refresh_bridge() {
    if let Ok(config_dir) = crate::providers::config_dir() {
        refresh_bridge_materialization(&config_dir);
    }
}

/// 内置技能随升级更新；不需要用户重新切换开关，也不会打开浏览器。
pub fn refresh_installed_skill(app: &AppHandle) {
    let outcome = (|| -> Result<()> {
        let config_dir = crate::providers::config_dir()?;
        let settings = load_settings_at(&settings_path_at(&config_dir))?;
        if !settings.enabled {
            return Ok(());
        }
        let path = config_dir.join(SKILL_DIR_NAME).join("SKILL.md");
        let builtin = include_str!("../../skills/browser-control/SKILL.md");
        let native_migration = cfg!(target_os = "macos") && settings.prefer_panel &&
            native_panel_entry_drifted(&mcp::load()?, &config_dir);
        if native_migration || std::fs::read_to_string(path).ok().as_deref() != Some(builtin) {
            let outcome = sync(&settings, &app.state::<SkillCenter>())?;
            for warning in outcome.warnings {
                eprintln!("[crewvela] 浏览器控制升级：{warning}");
            }
        }
        Ok(())
    })();
    if let Err(error) = outcome {
        eprintln!("[crewvela] 更新浏览器控制技能失败：{error:#}");
    }
}

fn native_panel_entry_drifted(servers: &[McpServerConfig], config_dir: &Path) -> bool {
    let bridge = bridge_path_at(config_dir).display().to_string();
    !servers.iter().any(|entry| entry.source_plugin.as_deref() == Some(BUILTIN_SOURCE_KEY)
        && entry.enabled && entry.args.first() == Some(&bridge))
}

pub(crate) fn build_mcp_config(
    settings: &BrowserControlSettings,
    browser: &ResolvedBrowser,
    engine: &EngineLaunch,
    bridge_path: Option<&Path>,
) -> McpServerConfig {
    let mut browser_flags = browser.engine_args.clone();
    // 隔离配置启动：默认持久 profile 在多会话并发时会锁冲突报错。
    // 面板接管模式下这批启动参数由桥接脚本剥掉（--isolated 会脱离面板页面）。
    browser_flags.push("--isolated".into());
    if settings.headless {
        browser_flags.push("--headless".into());
    }
    // 面板优先：入口换成桥接脚本，它按面板 CDP 通道存亡决定接管面板或透传兜底；
    // 形态 node <bridge> <engineCommand> <engineArgs...> -- <browserFlags...>。
    let (command, args) = if let Some(bridge) = bridge_path {
        let mut bridge_args = vec![bridge.display().to_string(), engine.command.clone()];
        bridge_args.extend(engine.args.iter().cloned());
        bridge_args.push("--".into());
        bridge_args.extend(browser_flags);
        (engine.node_path.clone(), bridge_args)
    } else {
        let mut direct = engine.args.clone();
        direct.extend(browser_flags);
        (engine.command.clone(), direct)
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default();
    McpServerConfig {
        id: uuid::Uuid::new_v4().to_string(),
        name: MCP_SERVER_NAME.into(),
        transport: McpTransport::Stdio,
        command: Some(command),
        args,
        env: BTreeMap::new(),
        url: None,
        headers: BTreeMap::new(),
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
        include_str!("../../skills/browser-control/SKILL.md"),
    )?;
    Ok(dir)
}

/// 清理同来源的历史版本资产（应用升级带来新 SKILL.md 时，旧资产及其部署先撤下）。
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

// ---- 同步主流程 ----

pub(crate) struct SyncOutcome {
    pub resolved_browser_label: String,
    pub engine_label: String,
    pub warnings: Vec<String>,
}

/// 按设置同步 MCP 条目与技能部署。返回解析结果与告警（告警不阻断开关）；
/// 关闭路径永不因浏览器/引擎缺失而失败。
pub(crate) fn sync_at(
    config_dir: &Path,
    mcp_file: &Path,
    center: &SkillCenter,
    settings: &BrowserControlSettings,
) -> Result<SyncOutcome> {
    let mut warnings = Vec::new();
    if !settings.enabled {
        mcp::remove_plugin_servers_at(mcp_file, BUILTIN_SOURCE_KEY)?;
        if let Err(error) = undeploy_skill_everywhere(center, &skill_source_ref(config_dir)) {
            warnings.push(format!("撤下浏览器控制技能失败：{error:#}"));
        }
        return Ok(SyncOutcome {
            resolved_browser_label: resolve_control_browser(settings)
                .map(|item| item.label)
                .unwrap_or_else(|_| "未解析（浏览器控制已关闭）".into()),
            engine_label: resolve_control_engine(settings)
                .map(|item| item.label)
                .unwrap_or_else(|_| "未解析（缺少 Node.js 或 @playwright/mcp）".into()),
            warnings,
        });
    }

    let resolved_browser = resolve_control_browser(settings)?;
    let engine = resolve_control_engine(settings)?;
    // 面板优先（Windows）：物化桥接脚本并让 MCP 条目走它；面板端口文件由
    // browser_panel 创建面板时写入，桥接在实际工具调用前请求激活并确定模式。
    let bridge_path = if settings.prefer_panel && cfg!(any(target_os = "windows", target_os = "macos")) {
        match materialize_bridge_at(config_dir) {
            Ok(path) => Some(path),
            Err(error) => {
                #[cfg(target_os = "macos")]
                return Err(error.context("无法更新 macOS 面板适配器，未替换现有 MCP 配置"));
                #[cfg(not(target_os = "macos"))]
                {
                warnings.push(format!("物化浏览器控制桥接脚本失败（退回直连模式）：{error:#}"));
                None
                }
            }
        }
    } else {
        None
    };
    let config = build_mcp_config(settings, &resolved_browser, &engine, bridge_path.as_deref());
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
        .map_err(|error| anyhow!("导入浏览器控制技能失败：{error:#}"))?;
    let asset = imported
        .first()
        .ok_or_else(|| anyhow!("导入浏览器控制技能失败：未识别到 SKILL.md"))?;
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
        warnings.push(format!("部署浏览器控制技能失败：{error:#}"));
    }
    if let Some(note) = &engine.note {
        warnings.push(note.clone());
    }
    Ok(SyncOutcome {
        resolved_browser_label: resolved_browser.label,
        engine_label: engine.label,
        warnings,
    })
}

fn skill_source_ref(config_dir: &Path) -> String {
    config_dir.join(SKILL_DIR_NAME).display().to_string()
}

fn sync(settings: &BrowserControlSettings, center: &SkillCenter) -> Result<SyncOutcome> {
    let config_dir = crate::providers::config_dir()?;
    sync_at(
        &config_dir,
        &config_dir.join("mcp-servers.json"),
        center,
        settings,
    )
}

/// 面板优先模式的面板状态行。None = 未启用面板优先 / 平台不支持。
/// 端口文件由 browser_panel 创建面板 webview 时写入。
fn summarize_panel(config_dir: &Path) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let _ = config_dir;
        Some("使用内置 WKWebView 面板；实际调用浏览器工具时自动显示。".into())
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let _ = config_dir;
        None
    }
    #[cfg(target_os = "windows")]
    {
        let raw = std::fs::read_to_string(crate::browser_panel::panel_cdp_file(config_dir)).ok()?;
        let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let port = value.get("port")?.as_u64()? as u16;
        if panel_port_listening(port) {
            Some(format!("内置面板已就绪（CDP 端口 {port}，Agent 将接管面板）"))
        } else {
            Some("面板未启动，实际调用浏览器工具时自动打开；连接失败时使用外部浏览器".into())
        }
    }
}

/// TCP 探活某个 CDP 端口。
pub(crate) fn panel_port_listening(port: u16) -> bool {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&address, std::time::Duration::from_millis(200)).is_ok()
}

/// CDP 本地 HTTP 探测使用现有 HTTP 客户端，拒绝重定向、错误状态、无效 JSON
/// 和过大的响应；仅收到任意 HTTP 响应头不代表页面可供接管。
fn panel_cdp_response(port: u16, route: &str) -> Option<Vec<u8>> {
    tauri::async_runtime::block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(std::time::Duration::from_millis(300))
            .timeout(std::time::Duration::from_millis(500))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .ok()?;
        let mut response = client
            .get(format!("http://127.0.0.1:{port}{route}"))
            .send()
            .await
            .ok()?;
        if response.status() != reqwest::StatusCode::OK {
            return None;
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.ok()? {
            if bytes.len() + chunk.len() > 128 * 1024 {
                return None;
            }
            bytes.extend_from_slice(&chunk);
        }
        Some(bytes)
    })
}

fn panel_cdp_json(port: u16, route: &str) -> Option<serde_json::Value> {
    serde_json::from_slice(&panel_cdp_response(port, route)?).ok()
}

/// port 只能来自本进程原生创建记录，不能来自可漂移的端口文件。
pub(crate) fn close_panel_cdp_pages(port: u16) -> Result<()> {
    let Some(targets) = panel_cdp_json(port, "/json/list") else {
        if !panel_port_listening(port) {
            return Ok(());
        }
        bail!("无法读取旧面板 CDP 页面列表");
    };
    let targets = targets
        .as_array()
        .ok_or_else(|| anyhow!("旧面板 CDP 页面列表无效"))?;
    for target in targets {
        if target.get("type").and_then(serde_json::Value::as_str) != Some("page") {
            continue;
        }
        let id = target
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| {
                !id.is_empty()
                    && id
                        .bytes()
                        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, b'-' | b'_'))
            })
            .ok_or_else(|| anyhow!("旧面板 CDP 页面 ID 无效"))?;
        if panel_cdp_response(port, &format!("/json/close/{id}")).is_none() {
            if !panel_port_listening(port) {
                return Ok(());
            }
            bail!("关闭旧面板 CDP 页面失败");
        }
    }
    Ok(())
}

pub(crate) fn panel_cdp_http_ready(port: u16) -> bool {
    panel_cdp_json(port, "/json/version").is_some_and(|value| value.is_object())
}

fn panel_cdp_port(config_dir: &Path) -> Option<u16> {
    let raw = std::fs::read_to_string(crate::browser_panel::panel_cdp_file(config_dir)).ok()?;
    let value: serde_json::Value = serde_json::from_str(&raw).ok()?;
    value
        .get("port")?
        .as_u64()
        .filter(|port| *port > 0 && *port <= u16::MAX as u64)
        .map(|port| port as u16)
}

fn panel_cdp_native_page_ready(port: u16, native_url: &str) -> bool {
    let Ok(expected) = reqwest::Url::parse(native_url) else {
        return false;
    };
    panel_cdp_json(port, "/json/list")
        .and_then(|value| value.as_array().cloned())
        .is_some_and(|targets| {
            targets.iter().any(|target| {
                target.get("type").and_then(serde_json::Value::as_str) == Some("page")
                    && target
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|id| !id.is_empty())
                    && target
                        .get("url")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|url| reqwest::Url::parse(url).ok())
                        == Some(expected.clone())
            })
        })
}

/// 重定向过程中原生 URL 与 CDP URL 可以短暂错开。只有同一个原生 URL
/// 持续未找到匹配页面才恢复控制器；URL 改变时重新计时。
#[derive(Default)]
struct PanelPageWait {
    missing: Option<(Option<String>, std::time::Instant)>,
}

impl PanelPageWait {
    fn should_recreate(
        &mut self,
        native_url: Option<&str>,
        ready: bool,
        now: std::time::Instant,
        grace: std::time::Duration,
    ) -> bool {
        if ready {
            self.missing = None;
            return false;
        }
        match &self.missing {
            Some((url, since)) if url.as_deref() == native_url => {
                now.duration_since(*since) >= grace
            }
            _ => {
                self.missing = Some((native_url.map(str::to_string), now));
                false
            }
        }
    }
}

/// 实际浏览器工具调用是否应打开内置面板。
pub(crate) fn should_auto_open_panel(settings: &BrowserControlSettings) -> bool {
    settings.enabled && settings.prefer_panel && cfg!(target_os = "windows")
}

/// 内置面板 CDP 是否就绪（端口文件 + HTTP /json/version 探活）。
pub(crate) fn panel_cdp_port_alive(config_dir: &Path) -> bool {
    #[cfg(target_os = "windows")]
    {
        panel_cdp_port(config_dir).is_some_and(panel_cdp_http_ready)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = config_dir;
        false
    }
}

/// 通知前端激活右侧栏浏览器标签的事件名（App.tsx 监听后 setBrowserOpen(true)，
/// 由 BrowserPanel 组件按标签容器矩形创建面板 webview 并写 CDP 端口文件）。
pub const PANEL_NEEDED_EVENT: &str = "browser-control-panel-needed";

/// 实际工具调用请求面板；启动聊天与 MCP 初始化不能进入此路径。
fn ensure_panel_online<R: tauri::Runtime>(app: &AppHandle<R>) -> Result<Option<u16>> {
    #[cfg(target_os = "windows")]
    {
        use tauri::Emitter;
        let config_dir = crate::providers::config_dir()?;
        let settings = load_settings_at(&settings_path_at(&config_dir))?;
        if !should_auto_open_panel(&settings) {
            return Ok(None);
        }
        // 即使 CDP 已在线也要激活右侧栏：面板可能被用户隐藏。
        app.emit(PANEL_NEEDED_EVENT, ())?;
        let (x, y, w, h) = default_panel_bounds(app);
        let existed = crate::browser_panel::panel_exists(app);
        let grace = std::time::Duration::from_millis(if existed { 1500 } else { 4000 });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(12);
        let mut wait = PanelPageWait::default();
        let mut recreated = false;
        while std::time::Instant::now() < deadline {
            if !crate::browser_panel::panel_exists(app) {
                if let Err(error) =
                    crate::browser_panel::open_panel_sync(app, "https://www.bing.com", x, y, w, h)
                {
                    eprintln!("[crewvela] 浏览器面板创建重试中：{error}");
                }
            }
            if crate::browser_panel::panel_exists(app) {
                // 已损坏的控制器仍可能留在 registry，但 url() 已报错。
                // None 也必须进入恢复，不能跳过后一直等到超时。
                let native_url = crate::browser_panel::panel_url(app);
                let port = panel_cdp_port(&config_dir);
                let ready = native_url.as_deref().is_some_and(|url| {
                    port.is_some_and(|port| panel_cdp_native_page_ready(port, url))
                });
                if ready {
                    return Ok(port);
                }
                if !recreated
                    && wait.should_recreate(
                        native_url.as_deref(),
                        ready,
                        std::time::Instant::now(),
                        grace,
                    )
                {
                    // CDP 中空页面/无窗口的新页面不能充当原生面板。保留安全 URL
                    // 与同一持久 profile，重建控制器使 MCP 改连新的真实面板端点。
                    let url = native_url
                        .as_deref()
                        .and_then(|url| reqwest::Url::parse(url).ok())
                        .filter(|url| matches!(url.scheme(), "http" | "https"))
                        .map(|url| url.to_string())
                        .unwrap_or_else(|| "https://www.bing.com".into());
                    let (x, y, w, h) =
                        crate::browser_panel::panel_bounds(app).unwrap_or((x, y, w, h));
                    crate::browser_panel::recreate_panel_sync(app, &url, x, y, w, h)
                        .map_err(|error| anyhow!("恢复浏览器面板失败：{error}"))?;
                    recreated = true;
                    wait = PanelPageWait::default();
                    app.emit(PANEL_NEEDED_EVENT, ())?;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
        bail!("等待原生面板页面就绪超时，使用外部浏览器")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = app;
        Ok(None)
    }
}

fn authorized_panel_request(header: &str, token: &str) -> bool {
    let mut lines = header.split("\r\n");
    if lines.next() != Some("POST /ensure HTTP/1.1") {
        return false;
    }
    // 只接受本地桥接携带的随机凭证；网页请求无法读取该本地配置。
    let authorization = format!("Bearer {token}");
    lines
        .filter_map(|line| line.split_once(':'))
        .any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value.trim() == authorization
        })
}

/// 桥接专用 loopback 服务。只收到认证的实际工具请求才创建/激活面板。
/// 单线程串行确保，不占用主线程，不复用协作服务或开放外网接口。
pub fn start_panel_request_service(app: &AppHandle, config_dir: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        crate::browser_panel_macos::start_service(app, config_dir)?;
        refresh_bridge_materialization(config_dir);
    }
    #[cfg(target_os = "windows")]
    {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let token = uuid::Uuid::new_v4().to_string();
        let directory = config_dir.join("browser-control");
        std::fs::create_dir_all(&directory)?;
        std::fs::write(
            directory.join("panel-request.json"),
            serde_json::to_vec(&serde_json::json!({ "port": port, "token": token }))?,
        )?;
        refresh_bridge_materialization(config_dir);
        let app = app.clone();
        std::thread::Builder::new().name("browser-panel-requests".into()).spawn(move || {
            for incoming in listener.incoming() {
                let Ok(mut stream) = incoming else { break };
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(std::time::Duration::from_secs(2)));
                let mut bytes = Vec::new();
                let mut chunk = [0u8; 512];
                while bytes.len() < 8192 && !bytes.windows(4).any(|part| part == b"\r\n\r\n") {
                    match stream.read(&mut chunk) {
                        Ok(0) | Err(_) => break,
                        Ok(count) => bytes.extend_from_slice(&chunk[..count]),
                    }
                }
                let authorized = bytes.len() <= 8192 && bytes.ends_with(b"\r\n\r\n")
                    && authorized_panel_request(&String::from_utf8_lossy(&bytes), &token);
                let (status, body) = if authorized {
                    match ensure_panel_online(&app) {
                        Ok(Some(port)) => ("200 OK", serde_json::json!({
                            "mode": "panel", "endpoint": format!("http://127.0.0.1:{port}")
                        })),
                        Ok(None) => ("200 OK", serde_json::json!({ "mode": "external" })),
                        Err(error) => {
                            eprintln!("[crewvela] 浏览器面板请求失败：{error:#}");
                            ("200 OK", serde_json::json!({ "mode": "external" }))
                        }
                    }
                } else {
                    ("403 Forbidden", serde_json::json!({ "error": "unauthorized" }))
                };
                let body = body.to_string();
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(response.as_bytes());
            }
        })?;
    }
    #[cfg(not(target_os = "windows"))]
    let _ = (app, config_dir);
    Ok(())
}

/// 默认面板位置：主窗口右栏 40% 宽（拿不到窗口尺寸时按 1440×900 兜底）。
fn default_panel_bounds<R: tauri::Runtime>(app: &AppHandle<R>) -> (f64, f64, f64, f64) {
    let (win_w, win_h) = app
        .get_window("main")
        .and_then(|window| window.inner_size().ok())
        .map(|size| (size.width as f64 / window_scale(&app), size.height as f64 / window_scale(&app)))
        .unwrap_or((1440.0, 900.0));
    let width = (win_w * 0.4).clamp(380.0, 720.0);
    (win_w - width, 64.0, width, (win_h - 110.0).max(360.0))
}

fn window_scale<R: tauri::Runtime>(app: &AppHandle<R>) -> f64 {
    app.get_window("main")
        .and_then(|window| window.scale_factor().ok())
        .unwrap_or(1.0)
}

/// 面板读取：设置 + 当前解析状态；若 MCP 条目与设置漂移（用户在 MCP 管理页
/// 手动停用/删除），以 browser-control.json 为准修复 enabled 状态。
fn status(app: &AppHandle) -> Result<BrowserControlStatus> {
    let config_dir = crate::providers::config_dir()?;
    let settings = load_settings_at(&settings_path_at(&config_dir))?;
    let center = app.state::<SkillCenter>();
    let panel = if settings.prefer_panel {
        summarize_panel(&config_dir)
    } else {
        None
    };
    if settings.enabled {
        // 自愈：条目缺失或被停用时按设置重建/恢复（浏览器/引擎参数取最新解析）。
        let servers = mcp::load().unwrap_or_default();
        let drifted = match servers
            .iter()
            .find(|server| server.source_plugin.as_deref() == Some(BUILTIN_SOURCE_KEY))
        {
            Some(entry) => !entry.enabled || (cfg!(target_os = "macos") && settings.prefer_panel &&
                native_panel_entry_drifted(&servers, &config_dir)),
            None => true,
        };
        if drifted {
            let outcome = sync(&settings, &center)?;
            return Ok(status_from(&settings, &outcome, panel));
        }
    }
    let resolved = resolve_control_browser(&settings).ok();
    let engine = resolve_control_engine(&settings).ok();
    Ok(BrowserControlStatus {
        settings: settings.clone(),
        resolved_browser: resolved.as_ref().map(|item| item.label.clone()),
        engine: engine
            .map(|item| item.label)
            .unwrap_or_else(|| "未解析（缺少 Node.js 或 @playwright/mcp）".into()),
        panel,
        warning: None,
    })
}

fn status_from(
    settings: &BrowserControlSettings,
    outcome: &SyncOutcome,
    panel: Option<String>,
) -> BrowserControlStatus {
    BrowserControlStatus {
        settings: settings.clone(),
        resolved_browser: Some(outcome.resolved_browser_label.clone()),
        engine: outcome.engine_label.clone(),
        panel,
        warning: outcome.warnings.first().cloned(),
    }
}

fn save(app: AppHandle, settings: BrowserControlSettings) -> Result<BrowserControlStatus> {
    if settings.browser == BrowserChannel::Custom && settings.custom_path.trim().is_empty() {
        bail!("选择自定义浏览器时必须填写可执行文件路径");
    }
    let config_dir = crate::providers::config_dir()?;
    let panel = if settings.prefer_panel {
        summarize_panel(&config_dir)
    } else {
        None
    };
    if !settings.enabled {
        // 关闭时不需要浏览器存在；仅做条目与技能清理。
        save_settings_at(&settings_path_at(&config_dir), &settings)?;
        let center = app.state::<SkillCenter>();
        let outcome = sync(&settings, &center)?;
        return Ok(status_from(&settings, &outcome, panel));
    }
    // 开启前先校验浏览器与引擎可解析，失败则不落盘（保持原状态）。
    resolve_control_browser(&settings)?;
    resolve_control_engine(&settings)?;
    save_settings_at(&settings_path_at(&config_dir), &settings)?;
    let center = app.state::<SkillCenter>();
    let outcome = sync(&settings, &center)?;
    Ok(status_from(&settings, &outcome, panel))
}

// ---- Tauri 命令 ----

#[tauri::command]
pub fn browser_control_get(app: AppHandle) -> Result<BrowserControlStatus, String> {
    status(&app).map_err(|error| format!("{error:#}"))
}

#[tauri::command]
pub fn browser_control_save(
    app: AppHandle,
    settings: BrowserControlSettings,
) -> Result<BrowserControlStatus, String> {
    save(app, settings).map_err(|error| format!("{error:#}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_panel_upgrade_detects_legacy_direct_entry_and_repairs_script_assets() {
        let dir = temp_dir();
        let browser = ResolvedBrowser { engine_args: vec![], label: "fixture".into() };
        let engine = EngineLaunch { command: "node".into(), args: vec!["playwright-cli.js".into()], node_path: "node".into(), label: "fixture".into(), note: None };
        let settings = BrowserControlSettings { enabled: true, ..Default::default() };
        let direct = build_mcp_config(&settings, &browser, &engine, None);
        assert!(native_panel_entry_drifted(&[direct], &dir));
        let bridge = materialize_bridge_at(&dir).unwrap();
        let entry = build_mcp_config(&settings, &browser, &engine, Some(&bridge));
        assert!(!native_panel_entry_drifted(&[entry], &dir));
        #[cfg(target_os = "macos")]
        {
            let dom = bridge.with_file_name("browser-panel-dom.mjs");
            std::fs::remove_file(&dom).unwrap();
            refresh_bridge_materialization(&dir);
            assert_eq!(std::fs::read_to_string(dom).unwrap(), include_str!("../resources/browser-panel-dom.mjs"));
            assert!(std::fs::read_to_string(bridge).unwrap().contains("crewvela-wkwebview"));
            let no_external_browser = BrowserControlSettings { browser: BrowserChannel::Custom, custom_path: "/missing-browser".into(), ..settings };
            assert_eq!(resolve_control_browser(&no_external_browser).unwrap().label, "内置 WKWebView");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn panel_request_requires_exact_route_and_bearer_token() {
        let token = "a-random-secret";
        let valid = "POST /ensure HTTP/1.1\r\nHost: 127.0.0.1\r\nAuthorization: Bearer a-random-secret\r\n\r\n";
        assert!(authorized_panel_request(valid, token));
        assert!(!authorized_panel_request(
            &valid.replace("POST", "GET"),
            token
        ));
        assert!(!authorized_panel_request(
            &valid.replace("/ensure", "/other"),
            token
        ));
        assert!(!authorized_panel_request(valid, "another-secret"));
        assert!(!authorized_panel_request(
            "POST /ensure HTTP/1.1\r\n\r\n",
            token
        ));
    }

    fn temp_dir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cc-browser-control-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn settings(enabled: bool, browser: BrowserChannel) -> BrowserControlSettings {
        BrowserControlSettings {
            enabled,
            browser,
            custom_path: String::new(),
            headless: false,
            prefer_panel: false,
        }
    }

    fn test_engine() -> EngineLaunch {
        EngineLaunch {
            command: "node.exe".into(),
            args: vec![r"C:\mcp\cli.js".into()],
            node_path: r"C:\Tools\node.exe".into(),
            label: String::new(),
            note: None,
        }
    }

    #[test]
    fn auto_open_panel_gates_on_enabled_and_prefer_panel() {
        let base = BrowserControlSettings {
            enabled: true,
            browser: BrowserChannel::Auto,
            custom_path: String::new(),
            headless: false,
            prefer_panel: true,
        };
        // 平台门控：Windows 上三条件齐才开；其他平台恒关。
        let expected = cfg!(target_os = "windows");
        assert_eq!(should_auto_open_panel(&base), expected);
        assert!(!should_auto_open_panel(&BrowserControlSettings { prefer_panel: false, ..base.clone() }));
        assert!(!should_auto_open_panel(&BrowserControlSettings { enabled: false, ..base }));
    }

    #[test]
    fn native_page_probe_rejects_hidden_replacement_empty_and_invalid_http() {
        fn serve(response: String) -> (u16, std::thread::JoinHandle<()>) {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let server = std::thread::spawn(move || {
                use std::io::{Read, Write};
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut buf = [0u8; 512];
                let count = stream.read(&mut buf).unwrap();
                assert!(String::from_utf8_lossy(&buf[..count]).starts_with("GET /json/list "));
                let _ = stream.write_all(response.as_bytes());
            });
            (port, server)
        }
        let page = r#"[{"type":"page","id":"native","url":"https://example.com/"}]"#;
        let cases = [
            ("200 OK", page, "https://example.com", true),
            ("200 OK", page, "https://example.com/redirected", false),
            (
                "200 OK",
                r#"[{"type":"page","id":"native","url":"https://example.com/redirected"}]"#,
                "https://example.com/redirected",
                true,
            ),
            ("200 OK", "[]", "https://example.com/", false),
            (
                "200 OK",
                r#"[{"type":"page","id":"hidden","url":"https://other.example/"}]"#,
                "https://example.com/",
                false,
            ),
            (
                "200 OK",
                r#"[{"type":"worker","id":"worker","url":"https://example.com/"}]"#,
                "https://example.com/",
                false,
            ),
            (
                "200 OK",
                r#"[{"type":"page","url":"https://example.com/"}]"#,
                "https://example.com/",
                false,
            ),
            ("404 Not Found", page, "https://example.com/", false),
            ("200 OK", "broken json", "https://example.com/", false),
        ];
        for (status, body, native_url, expected) in cases {
            let (port, server) = serve(format!(
                "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            ));
            assert_eq!(
                panel_cdp_native_page_ready(port, native_url),
                expected,
                "{status} {body}"
            );
            server.join().unwrap();
        }
        let body = " ".repeat(128 * 1024 + 1);
        let (port, server) = serve(format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ));
        assert!(!panel_cdp_native_page_ready(port, "https://example.com/"));
        server.join().unwrap();
    }

    #[test]
    fn missing_native_page_wait_resets_on_redirect_and_readiness() {
        let start = std::time::Instant::now();
        let grace = std::time::Duration::from_millis(1500);
        let mut wait = PanelPageWait::default();
        assert!(!wait.should_recreate(Some("https://example.com/"), false, start, grace));
        assert!(!wait.should_recreate(
            Some("https://example.com/"),
            false,
            start + grace / 2,
            grace
        ));
        // 原生导航切到重定向 URL 时，不能把之前的失配累计到新页面。
        assert!(!wait.should_recreate(
            Some("https://example.com/redirected"),
            false,
            start + grace,
            grace
        ));
        assert!(!wait.should_recreate(
            Some("https://example.com/redirected"),
            true,
            start + grace * 2,
            grace
        ));
        assert!(!wait.should_recreate(
            Some("https://example.com/redirected"),
            false,
            start + grace * 3,
            grace
        ));
        assert!(wait.should_recreate(
            Some("https://example.com/redirected"),
            false,
            start + grace * 4,
            grace
        ));
    }

    #[test]
    fn disconnected_native_controller_without_url_enters_recovery() {
        let start = std::time::Instant::now();
        let grace = std::time::Duration::from_millis(1500);
        let mut wait = PanelPageWait::default();
        assert!(!wait.should_recreate(None, false, start, grace));
        assert!(!wait.should_recreate(None, false, start + grace / 2, grace));
        assert!(wait.should_recreate(None, false, start + grace, grace));
        // 冷启动控制器从 URL 不可读到可读时，重新给予完整的页面等待期。
        assert!(!wait.should_recreate(Some("https://example.com/"), false, start + grace, grace));
        assert!(!wait.should_recreate(
            Some("https://example.com/"),
            true,
            start + grace * 2,
            grace
        ));
        assert!(!wait.should_recreate(None, false, start + grace * 3, grace));
        assert!(wait.should_recreate(None, false, start + grace * 4, grace));
    }

    #[test]
    fn panel_cleanup_closes_only_pages_and_rejects_path_ids_and_http_errors() {
        fn run(list: &str, close_status: &str, expect_close: bool) -> bool {
            use std::io::{Read, Write};
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            let list = list.to_string();
            let close_status = close_status.to_string();
            let (release, released) = std::sync::mpsc::channel();
            let server = std::thread::spawn(move || {
                let mut buf = [0u8; 1024];
                let (mut stream, _) = listener.accept().unwrap();
                let count = stream.read(&mut buf).unwrap();
                assert!(String::from_utf8_lossy(&buf[..count]).starts_with("GET /json/list "));
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{list}",
                    list.len()
                )
                .unwrap();
                drop(stream);
                if expect_close {
                    let (mut stream, _) = listener.accept().unwrap();
                    let count = stream.read(&mut buf).unwrap();
                    assert!(String::from_utf8_lossy(&buf[..count])
                        .starts_with("GET /json/close/page-1 "));
                    write!(stream, "HTTP/1.1 {close_status}\r\nContent-Length: 17\r\nConnection: close\r\n\r\nTarget is closing").unwrap();
                }
                // 错误状态时端口仍活着；不能把 HTTP 错误伪装成已退出。
                released.recv().unwrap();
            });
            let ok = close_panel_cdp_pages(port).is_ok();
            release.send(()).unwrap();
            server.join().unwrap();
            ok
        }
        let list = r#"[{"type":"worker","id":"worker"},{"type":"page","id":"page-1"}]"#;
        assert!(run(list, "200 OK", true));
        assert!(!run(list, "404 Not Found", true));
        assert!(!run(
            r#"[{"type":"page","id":"../foreign"}]"#,
            "200 OK",
            false
        ));
        assert!(!run(r#"[{"type":"page","id":""}]"#, "200 OK", false));
        assert!(run(r#"[{"type":"worker","id":"worker"}]"#, "200 OK", false));
    }

    #[test]
    fn panel_cdp_port_alive_requires_live_listener() {
        let dir = temp_dir();
        // 无端口文件 → 离线。
        assert!(!panel_cdp_port_alive(&dir));
        // 生产环境中 browser-control 目录由 sync 物化 bridge 时创建。
        std::fs::create_dir_all(dir.join("browser-control")).unwrap();
        // 死端口（无人监听）→ 离线。
        std::fs::write(
            crate::browser_panel::panel_cdp_file(&dir),
            r#"{"port":59999,"startedAt":0}"#,
        )
        .unwrap();
        assert!(!panel_cdp_port_alive(&dir));
        // 真 CDP 形态（HTTP /json/version 应答）→ 在线。
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            use std::io::{Read, Write};
            // Probe HTTP on every platform; the panel wrapper is Windows-only.
            for _ in 0..(1 + usize::from(cfg!(target_os = "windows"))) {
                let (mut stream, _) = listener.accept().unwrap();
                stream.set_read_timeout(Some(std::time::Duration::from_secs(2))).unwrap();
                let mut buf = [0u8; 256];
                let count = stream.read(&mut buf).unwrap();
                assert!(String::from_utf8_lossy(&buf[..count]).starts_with("GET /json/version "));
                stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").unwrap();
            }
        });
        std::fs::write(
            crate::browser_panel::panel_cdp_file(&dir),
            format!(r#"{{"port":{port},"startedAt":0}}"#),
        )
        .unwrap();
        assert_eq!(panel_cdp_port(&dir), Some(port));
        assert!(panel_cdp_http_ready(port));
        assert_eq!(panel_cdp_port_alive(&dir), cfg!(target_os = "windows"));
        server.join().unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn settings_roundtrip_and_default() {
        let dir = temp_dir();
        let path = settings_path_at(&dir);
        assert_eq!(load_settings_at(&path).unwrap(), BrowserControlSettings::default());
        assert!(BrowserControlSettings::default().prefer_panel, "面板优先默认开启");
        // 旧配置文件没有 preferPanel 字段 → 反序列化取默认 true。
        std::fs::write(&path, r#"{"enabled":true,"browser":"auto","customPath":"","headless":false}"#).unwrap();
        assert!(load_settings_at(&path).unwrap().prefer_panel);
        let mut custom = settings(true, BrowserChannel::Custom);
        custom.custom_path = r"C:\browsers\chrome.exe".into();
        custom.headless = true;
        custom.prefer_panel = true;
        save_settings_at(&path, &custom).unwrap();
        assert_eq!(load_settings_at(&path).unwrap(), custom);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn resolve_browser_custom_requires_existing_file() {
        let dir = temp_dir();
        let exe = dir.join("chrome.exe");
        std::fs::write(&exe, b"fake").unwrap();
        let mut custom = settings(true, BrowserChannel::Custom);
        custom.custom_path = exe.display().to_string();
        let resolved = resolve_browser(&custom).unwrap();
        assert_eq!(
            resolved.engine_args,
            vec![
                "--executable-path".to_string(),
                exe.display().to_string()
            ]
        );
        custom.custom_path = dir.join("missing.exe").display().to_string();
        assert!(resolve_browser(&custom).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mcp_config_carries_isolated_and_engine_prefix() {
        let browser = ResolvedBrowser {
            engine_args: vec!["--browser".into(), "msedge".into()],
            label: String::new(),
        };
        let engine = test_engine();
        let config = build_mcp_config(
            &settings(true, BrowserChannel::Auto),
            &browser,
            &engine,
            None,
        );
        assert_eq!(config.name, "browser-control");
        assert_eq!(config.transport, McpTransport::Stdio);
        assert_eq!(config.source_plugin.as_deref(), Some(BUILTIN_SOURCE_KEY));
        assert!(config.enabled);
        assert_eq!(config.command.as_deref(), Some("node.exe"));
        assert_eq!(
            config.args,
            vec![
                r"C:\mcp\cli.js".to_string(),
                "--browser".to_string(),
                "msedge".to_string(),
                "--isolated".to_string(),
            ]
        );
        let mut headless = settings(true, BrowserChannel::Auto);
        headless.headless = true;
        let config = build_mcp_config(&headless, &browser, &engine, None);
        assert_eq!(config.args.last().unwrap(), "--headless");
    }

    #[test]
    fn mcp_config_bridge_form_when_prefer_panel() {
        // 面板优先：node <bridge> <engineCommand> <engineArgs...> -- <browserFlags...>。
        let browser = ResolvedBrowser {
            engine_args: vec!["--browser".into(), "chrome".into()],
            label: String::new(),
        };
        let engine = test_engine();
        let mut prefer = settings(true, BrowserChannel::Auto);
        prefer.prefer_panel = true;
        let bridge = Path::new(r"C:\conf\browser-control\bridge.mjs");
        let config = build_mcp_config(&prefer, &browser, &engine, Some(bridge));
        assert_eq!(config.command.as_deref(), Some(r"C:\Tools\node.exe"));
        assert_eq!(
            config.args,
            vec![
                r"C:\conf\browser-control\bridge.mjs".to_string(),
                "node.exe".to_string(),
                r"C:\mcp\cli.js".to_string(),
                "--".to_string(),
                "--browser".to_string(),
                "chrome".to_string(),
                "--isolated".to_string(),
            ]
        );
    }

    #[test]
    fn materialize_bridge_writes_runnable_script() {
        let dir = temp_dir();
        let bridge = materialize_bridge_at(&dir).unwrap();
        let content = std::fs::read_to_string(&bridge).unwrap();
        if cfg!(target_os = "macos") {
            assert!(content.contains("MacOSPanelBridge"));
            assert!(bridge.with_file_name("browser-panel-dom.mjs").is_file());
        } else {
            assert!(content.contains("parseBridgeArgs"));
        }
        assert_eq!(bridge, bridge_path_at(&dir));
        // 端口文件路径与面板写入位置一致（bridge 默认取同目录 panel-cdp.json）。
        assert_eq!(
            bridge.parent().unwrap().join("panel-cdp.json"),
            crate::browser_panel::panel_cdp_file(&dir)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn builtin_upsert_is_inplace_and_rejects_name_clash() {
        let dir = temp_dir();
        let mcp_file = dir.join("mcp-servers.json");
        let browser = ResolvedBrowser { engine_args: vec![], label: String::new() };
        let engine = EngineLaunch {
            command: "node".into(),
            args: vec!["cli.js".into()],
            node_path: "node".into(),
            label: String::new(),
            note: None,
        };
        let first = build_mcp_config(&settings(true, BrowserChannel::Auto), &browser, &engine, None);
        let saved = mcp::upsert_builtin_server_at(&mcp_file, BUILTIN_SOURCE_KEY, first.clone()).unwrap();
        // 再 upsert 变更参数：原位更新，id 不变。
        let mut updated_args = first.args.clone();
        updated_args.push("--headless".into());
        let mut second = first.clone();
        second.args = updated_args;
        let saved_again =
            mcp::upsert_builtin_server_at(&mcp_file, BUILTIN_SOURCE_KEY, second).unwrap();
        assert_eq!(saved_again.id, saved.id);
        let servers = mcp::load_at(&mcp_file).unwrap();
        assert_eq!(servers.len(), 1);
        assert!(servers[0].args.contains(&"--headless".to_string()));
        // 用户手配同名条目 → 拒绝。
        let mut clash = build_mcp_config(&settings(true, BrowserChannel::Auto), &browser, &engine, None);
        clash.source_plugin = None;
        assert!(mcp::upsert_builtin_server_at(&mcp_file, "other@builtin", clash).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sync_disable_removes_entry_and_deployments() {
        let dir = temp_dir();
        let mcp_file = dir.join("mcp-servers.json");
        let center = SkillCenter::open_at(dir.join("skill-center")).unwrap();

        // 预置：启用态的内置条目 + 一个已部署技能。部署用 project 作用域指向
        // 沙箱目录，避免测试触碰真实 ~/.claude/skills。
        let browser = ResolvedBrowser { engine_args: vec![], label: String::new() };
        let engine = EngineLaunch {
            command: "node".into(),
            args: vec![],
            node_path: "node".into(),
            label: String::new(),
            note: None,
        };
        let entry = build_mcp_config(&settings(true, BrowserChannel::Auto), &browser, &engine, None);
        mcp::upsert_builtin_server_at(&mcp_file, BUILTIN_SOURCE_KEY, entry).unwrap();

        let source = materialize_skill_at(&dir).unwrap();
        assert!(source.join("SKILL.md").is_file());
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

        let disabled = settings(false, BrowserChannel::Auto);
        let outcome = sync_at(&dir, &mcp_file, &center, &disabled).unwrap();
        assert!(outcome.warnings.is_empty());
        assert!(mcp::load_at(&mcp_file).unwrap().is_empty());
        assert!(center
            .asset_ids_by_source_ref(&source.display().to_string())
            .unwrap()
            .is_empty());
        assert!(center.deployment_ids_for_skill(&asset_id).unwrap().is_empty());
        // SQLite 连接持句柄，先释放再清理沙箱。
        drop(center);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn native_bridge_write_failure_keeps_existing_mcp_entry() {
        let dir = temp_dir();
        let file = dir.join("mcp-servers.json");
        let center = SkillCenter::open_at(dir.join("skill-center")).unwrap();
        let settings = BrowserControlSettings { enabled: true, ..Default::default() };
        let browser = ResolvedBrowser { engine_args: vec![], label: "fixture".into() };
        let engine = EngineLaunch { command: "node".into(), args: vec!["old-playwright.js".into()], node_path: "node".into(), label: "fixture".into(), note: None };
        let existing = build_mcp_config(&settings, &browser, &engine, None);
        mcp::upsert_builtin_server_at(&file, BUILTIN_SOURCE_KEY, existing).unwrap();
        let before = std::fs::read(&file).unwrap();
        std::fs::create_dir_all(bridge_path_at(&dir)).unwrap();
        assert!(sync_at(&dir, &file, &center, &settings).is_err());
        assert_eq!(std::fs::read(&file).unwrap(), before);
        drop(center);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
