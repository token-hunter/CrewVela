// MCP（Model Context Protocol）服务器管理：配置持久化、连通性探测、
// 以及把启用的 server 注入交互式 claude 会话的 --settings mcpServers。
// 参考 ZCode 的 MCP 管理：卡片列表（传输类型/状态/工具数/开关）+ stdio/sse/http 三类接入。

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// npx 类 stdio server 冷启动可能要下载依赖，探测超时给足。
const STDIO_PROBE_TIMEOUT: Duration = Duration::from_secs(45);
const REMOTE_PROBE_TIMEOUT: Duration = Duration::from_secs(15);
static CONFIG_WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn config_write_lock() -> Result<std::sync::MutexGuard<'static, ()>> {
    CONFIG_WRITES.lock().map_err(|_| anyhow!("MCP 配置写入锁已损坏"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpTransport {
    Stdio,
    Sse,
    Http,
}

impl McpTransport {
    pub fn label(self) -> &'static str {
        match self {
            Self::Stdio => "STDIO",
            Self::Sse => "SSE",
            Self::Http => "HTTP",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// 来源插件（"plugin@marketplace"），插件市场导入的条目凭它反向清理。
    #[serde(default)]
    pub source_plugin: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpUpsertInput {
    pub id: Option<String>,
    pub name: String,
    pub transport: McpTransport,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub source_plugin: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolInfo {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpProbeReport {
    pub id: String,
    pub ok: bool,
    /// connected | failed
    pub status: &'static str,
    pub tools: Vec<McpToolInfo>,
    pub error: Option<String>,
    pub probed_at: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct McpFile {
    version: u32,
    #[serde(default)]
    servers: Vec<McpServerConfig>,
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn mcp_path() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("mcp-servers.json"))
}

pub fn load() -> Result<Vec<McpServerConfig>> {
    let mut servers = load_at(&mcp_path()?)?;
    if let Ok(executable) = std::env::current_exe() {
        refresh_owned_launcher_commands(&mut servers, &executable);
    }
    Ok(servers)
}

// Built-in launcher commands follow the running binary after a portable move or
// rename. Keep custom servers, arguments and the user's enabled state intact.
fn refresh_owned_launcher_commands(servers: &mut [McpServerConfig], executable: &Path) {
    for server in servers {
        let previous = server.command.as_deref().and_then(|command| Path::new(command).file_name()).and_then(|name| name.to_str());
        if server.source_plugin.as_deref() == Some(crate::computer_control::BUILTIN_SOURCE_KEY)
            && server.transport == McpTransport::Stdio
            && server.args.first().is_some_and(|arg| arg == "--computer-control-mcp")
            && previous.is_some_and(crate::brand::is_launcher_name) {
            server.command = Some(executable.display().to_string());
        }
    }
}

pub(crate) fn load_at(path: &Path) -> Result<Vec<McpServerConfig>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("读取 MCP 配置失败: {}", path.display()))?;
    let file: McpFile = serde_json::from_str(&raw).context("解析 mcp-servers.json 失败")?;
    Ok(file.servers)
}

fn save_at(path: &Path, servers: &[McpServerConfig]) -> Result<()> {
    let file = McpFile {
        version: 1,
        servers: servers.to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&file).context("序列化 MCP 配置失败")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, path)?;
    Ok(())
}

fn validate(input: &McpUpsertInput, existing: &[McpServerConfig]) -> Result<McpServerConfig> {
    let name = input.name.trim();
    if name.is_empty() {
        bail!("服务器名称不能为空");
    }
    if name.chars().count() > 64 {
        bail!("服务器名称不能超过 64 个字符");
    }
    if existing
        .iter()
        .any(|server| server.name == name && server.id != input.id.clone().unwrap_or_default())
    {
        bail!("已存在同名 MCP 服务器：{name}");
    }
    match input.transport {
        McpTransport::Stdio => {
            if !input
                .command
                .as_deref()
                .is_some_and(|command| !command.trim().is_empty())
            {
                bail!("STDIO 服务器必须填写启动命令");
            }
            if input.url.as_deref().is_some_and(|url| !url.trim().is_empty()) {
                bail!("STDIO 服务器不需要填写 URL");
            }
        }
        McpTransport::Http | McpTransport::Sse => {
            let url = input
                .url
                .as_deref()
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .ok_or_else(|| anyhow!("{} 服务器必须填写 URL", input.transport.label()))?;
            if !url.starts_with("http://") && !url.starts_with("https://") {
                bail!("URL 必须以 http:// 或 https:// 开头");
            }
            if input
                .command
                .as_deref()
                .is_some_and(|command| !command.trim().is_empty())
            {
                bail!("远程服务器不需要填写启动命令");
            }
        }
    }
    for (key, value) in &input.env {
        if key.trim().is_empty() {
            bail!("环境变量的名称不能为空");
        }
        if value.contains('\n') {
            bail!("环境变量的值不能包含换行");
        }
    }
    for (key, value) in &input.headers {
        if key.trim().is_empty() {
            bail!("请求头的名称不能为空");
        }
        if value.contains('\n') {
            bail!("请求头的值不能包含换行");
        }
    }

    let now = now_millis();
    Ok(McpServerConfig {
        id: input.id.clone().unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
        name: name.to_string(),
        transport: input.transport,
        command: input
            .command
            .as_deref()
            .map(str::trim)
            .filter(|command| !command.is_empty())
            .map(str::to_string),
        args: input.args.clone(),
        env: input.env.clone(),
        url: input
            .url
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(str::to_string),
        headers: input.headers.clone(),
        enabled: input.enabled,
        source_plugin: input.source_plugin.clone(),
        created_at: now,
        updated_at: now,
    })
}

pub fn upsert(input: McpUpsertInput) -> Result<McpServerConfig> {
    let _write = config_write_lock()?;
    let path = mcp_path()?;
    let mut servers = load_at(&path)?;
    let mut validated = validate(&input, &servers)?;
    let saved = match servers
        .iter_mut()
        .find(|server| server.id == validated.id)
    {
        Some(existing) => {
            validated.created_at = existing.created_at;
            *existing = validated.clone();
            existing.clone()
        }
        None => {
            servers.push(validated.clone());
            validated
        }
    };
    save_at(&path, &servers)?;
    Ok(saved)
}

pub fn delete(id: &str) -> Result<()> {
    let _write = config_write_lock()?;
    let path = mcp_path()?;
    let mut servers = load_at(&path)?;
    let before = servers.len();
    servers.retain(|server| server.id != id);
    if servers.len() == before {
        bail!("未找到 MCP 服务器: {id}");
    }
    save_at(&path, &servers)
}

pub fn set_enabled(id: &str, enabled: bool) -> Result<McpServerConfig> {
    let _write = config_write_lock()?;
    let path = mcp_path()?;
    let mut servers = load_at(&path)?;
    let server = servers
        .iter_mut()
        .find(|server| server.id == id)
        .ok_or_else(|| anyhow!("未找到 MCP 服务器: {id}"))?;
    server.enabled = enabled;
    server.updated_at = now_millis();
    let updated = server.clone();
    save_at(&path, &servers)?;
    Ok(updated)
}

/// 转成 claude settings.json 的 mcpServers 单项格式。
pub fn claude_settings_entry(server: &McpServerConfig) -> Value {
    match server.transport {
        McpTransport::Stdio => {
            let mut entry = json!({
                "command": server.command.clone().unwrap_or_default(),
            });
            if !server.args.is_empty() {
                entry["args"] = json!(server.args);
            }
            if !server.env.is_empty() {
                entry["env"] = json!(server.env);
            }
            entry
        }
        McpTransport::Http | McpTransport::Sse => {
            let kind = match server.transport {
                McpTransport::Http => "http",
                _ => "sse",
            };
            let mut entry = json!({
                "type": kind,
                "url": server.url.clone().unwrap_or_default(),
            });
            if !server.headers.is_empty() {
                entry["headers"] = json!(server.headers);
            }
            entry
        }
    }
}

/// 启用的 server 按名称映射成 mcpServers 对象（会话启动时注入 --settings）。
pub fn enabled_claude_map() -> Result<serde_json::Map<String, Value>> {
    let mut map = serde_json::Map::new();
    for server in load()? {
        if server.enabled {
            map.insert(server.name.clone(), claude_settings_entry(&server));
        }
    }
    Ok(map)
}

/// 单个 server 转 ACP session 的 mcpServers 条目。
/// ACP v1 两种形态：http（带 type:"http"，headers 为 [{name,value}] 数组）
/// 与 stdio（{name, command, args, env}，env 同为数组形态）。
/// SSE 保持 None：ACP 只协商 http 能力，SSE server 继续走 claude 链路。
pub fn acp_server_entry(server: &McpServerConfig) -> Option<Value> {
    match server.transport {
        McpTransport::Http => {
            let mut entry = json!({
                "type": "http",
                "name": server.name,
                "url": server.url.clone().unwrap_or_default(),
            });
            if !server.headers.is_empty() {
                entry["headers"] = name_value_array(&server.headers);
            }
            Some(entry)
        }
        McpTransport::Stdio => {
            // ACP SDK 1.3 的 McpServerStdio schema 把 args/env 都列为 required，
            // 缺字段会被 x-deserialize-skip-invalid-items 静默丢弃（适配器收到空
            // mcpServers，claude 进程不会带 --mcp-config）。因此空也要显式输出 []。
            Some(json!({
                "name": server.name,
                "command": server.command.clone().unwrap_or_default(),
                "args": server.args,
                "env": name_value_array(&server.env),
            }))
        }
        McpTransport::Sse => None,
    }
}

/// BTreeMap → ACP 的 [{name,value}] 数组形态。
fn name_value_array(map: &BTreeMap<String, String>) -> Value {
    Value::Array(
        map.iter()
            .map(|(name, value)| json!({ "name": name, "value": value }))
            .collect(),
    )
}

/// 启用的 server（http + stdio）列表（多 Agent 的 ACP 执行路径追加用）。
pub fn enabled_acp_servers() -> Result<Vec<Value>> {
    Ok(load()?
        .into_iter()
        .filter(|server| server.enabled)
        .filter_map(|server| acp_server_entry(&server))
        .collect())
}

/// 原生聊天 ACP 会话（claude/codex/kimi/dsh chat）注入的 server：仅 stdio。
/// http 条目需要 agent 侧 mcpCapabilities.http 能力协商（各适配器支持不一），
/// sse 不在 ACP 形态内，均跳过；本地 stdio server（如浏览器控制）借此进入全部原生聊天。
pub fn enabled_native_chat_servers() -> Result<Vec<Value>> {
    Ok(load()?
        .into_iter()
        .filter(|server| server.enabled && server.transport == McpTransport::Stdio)
        .filter_map(|server| acp_server_entry(&server))
        .collect())
}

/// codex PTY 独立会话的 MCP 注入：启用的 stdio server 逐个转成一条
/// `mcp_servers."<name>".<field>=<TOML 值>` 覆盖（codex 的 -c 约定，值按 TOML 解析）。
/// 读取失败或字段缺失的条目静默跳过，不阻塞会话启动。
pub fn codex_cli_mcp_overrides() -> Vec<String> {
    let Ok(path) = mcp_path() else {
        return Vec::new();
    };
    codex_cli_mcp_overrides_at(&path)
}

pub(crate) fn codex_cli_mcp_overrides_at(path: &Path) -> Vec<String> {
    let Ok(servers) = load_at(path) else {
        return Vec::new();
    };
    servers
        .into_iter()
        .filter(|server| server.enabled && server.transport == McpTransport::Stdio)
        .filter_map(|server| {
            let command = server.command.as_deref()?.trim();
            if command.is_empty() {
                return None;
            }
            // 表名里的引号不安全，替换掉；正常命名（browser-control 等）原样保留。
            let key = format!("mcp_servers.\"{}\"", server.name.replace('"', "_"));
            let mut overrides = vec![
                format!("{key}.command={}", toml_value(&TomsValueForCodex::Str(command))),
                format!(
                    "{key}.args={}",
                    toml_value(&TomsValueForCodex::Array(&server.args))
                ),
            ];
            if !server.env.is_empty() {
                overrides.push(format!(
                    "{key}.env={}",
                    toml_value(&TomsValueForCodex::Table(&server.env))
                ));
            }
            Some(overrides)
        })
        .flatten()
        .collect()
}

/// 序列化为单个 TOML 值（codex -c 的值语法）。覆盖本项目用到的三种形态：
/// 字符串 / 字符串数组 / 字符串表。Windows 路径的反斜杠必须转义。
fn toml_value(value: &TomsValueForCodex<'_>) -> String {
    match value {
        TomsValueForCodex::Str(text) => toml_string(text),
        TomsValueForCodex::Array(items) => format!(
            "[{}]",
            items
                .iter()
                .map(|item| toml_string(item))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TomsValueForCodex::Table(entries) => format!(
            "{{ {} }}",
            entries
                .iter()
                .map(|(key, value)| format!("{} = {}", toml_string(key), toml_string(value)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

enum TomsValueForCodex<'a> {
    Str(&'a str),
    Array(&'a [String]),
    Table(&'a BTreeMap<String, String>),
}

fn toml_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => out.push_str(&format!("\\u{:04X}", ch as u32)),
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}

// ---- 内置功能条目（浏览器控制等）：source_plugin 即命名空间 ----

/// 内置功能条目 upsert：按 source_plugin 原位更新（保留 id/created_at；enabled
/// 由内置功能自身同步），找不到则新增。与插件导入不同，名称不加前缀。
/// 用户若已手配同名条目则拒绝，避免 agent 侧出现重名 server。
pub(crate) fn upsert_builtin_server_at(
    path: &Path,
    source_key: &str,
    mut config: McpServerConfig,
) -> Result<McpServerConfig> {
    let _write = config_write_lock()?;
    let mut servers = load_at(path)?;
    if let Some(clash) = servers
        .iter()
        .find(|server| server.name == config.name && server.source_plugin.as_deref() != Some(source_key))
    {
        bail!(
            "已存在同名 MCP 服务器：{}（用户手动配置），内置功能无法注册，请先改名或删除该条目",
            clash.name
        );
    }
    match servers
        .iter_mut()
        .find(|server| server.source_plugin.as_deref() == Some(source_key))
    {
        Some(existing) => {
            config.id = existing.id.clone();
            config.created_at = existing.created_at;
            config.updated_at = now_millis();
            *existing = config.clone();
        }
        None => servers.push(config.clone()),
    }
    save_at(path, &servers)?;
    Ok(config)
}

pub fn upsert_builtin_server(source_key: &str, config: McpServerConfig) -> Result<McpServerConfig> {
    upsert_builtin_server_at(&mcp_path()?, source_key, config)
}

// ---- 插件市场集成：按来源插件批量导入/清理 ----

/// 解析插件 `.mcp.json` 的单个 server 定义（claude settings mcpServers 子项格式）。
fn server_config_from_json(
    name: String,
    value: &Value,
    source_plugin: &str,
    now: u64,
) -> Result<McpServerConfig> {
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    let (transport, command, url) = match kind {
        "http" | "sse" => {
            let url = value
                .get("url")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .ok_or_else(|| anyhow!("MCP server {name} 缺少 url"))?;
            (
                if kind == "http" {
                    McpTransport::Http
                } else {
                    McpTransport::Sse
                },
                None,
                Some(url.to_string()),
            )
        }
        _ => {
            let command = value
                .get("command")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|command| !command.is_empty())
                .ok_or_else(|| anyhow!("MCP server {name} 缺少 command 或 url"))?;
            (McpTransport::Stdio, Some(command.to_string()), None)
        }
    };
    let args = value
        .get("args")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let string_map = |key: &str| -> BTreeMap<String, String> {
        value
            .get(key)
            .and_then(Value::as_object)
            .map(|object| {
                object
                    .iter()
                    .filter_map(|(k, v)| {
                        v.as_str().map(|value| (k.clone(), value.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    Ok(McpServerConfig {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        transport,
        command,
        args,
        env: string_map("env"),
        url,
        headers: string_map("headers"),
        enabled: true,
        source_plugin: Some(source_plugin.to_string()),
        created_at: now,
        updated_at: now,
    })
}

pub(crate) fn import_plugin_servers_at(
    path: &Path,
    plugin_key: &str,
    plugin_name: &str,
    servers: &serde_json::Map<String, Value>,
) -> Result<Vec<McpServerConfig>> {
    if servers.is_empty() {
        return Ok(Vec::new());
    }
    let _write = config_write_lock()?;
    let mut existing = load_at(path)?;
    let now = now_millis();
    let mut imported = Vec::new();
    for (server_name, definition) in servers {
        if !definition.is_object() {
            bail!("MCP server {server_name} 定义不是对象");
        }
        // 命名加插件前缀避免与用户手配冲突；同插件重装时按（来源插件 + 名称）原位更新。
        let target_name = format!("{plugin_name}-{server_name}");
        let mut config = server_config_from_json(target_name, definition, plugin_key, now)?;
        if let Some(slot) = existing
            .iter_mut()
            .find(|item| item.source_plugin.as_deref() == Some(plugin_key) && item.name == config.name)
        {
            config.id = slot.id.clone();
            config.enabled = slot.enabled;
            config.created_at = slot.created_at;
            *slot = config.clone();
        } else {
            existing.push(config.clone());
        }
        imported.push(config);
    }
    save_at(path, &existing)?;
    Ok(imported)
}

/// 导入插件捆绑的 MCP servers（默认启用；名称带插件前缀）。
pub fn import_plugin_servers(
    plugin_key: &str,
    plugin_name: &str,
    servers: &serde_json::Map<String, Value>,
) -> Result<Vec<McpServerConfig>> {
    import_plugin_servers_at(&mcp_path()?, plugin_key, plugin_name, servers)
}

pub(crate) fn remove_plugin_servers_at(path: &Path, plugin_key: &str) -> Result<usize> {
    let _write = config_write_lock()?;
    let mut servers = load_at(path)?;
    let before = servers.len();
    servers.retain(|server| server.source_plugin.as_deref() != Some(plugin_key));
    let removed = before - servers.len();
    if removed > 0 {
        save_at(path, &servers)?;
    }
    Ok(removed)
}

/// 移除某插件导入的全部 MCP servers，返回移除数量。
pub fn remove_plugin_servers(plugin_key: &str) -> Result<usize> {
    remove_plugin_servers_at(&mcp_path()?, plugin_key)
}

// ---- 连通性探测 ----

fn probe_report(id: &str, ok: bool, tools: Vec<McpToolInfo>, error: Option<String>) -> McpProbeReport {
    McpProbeReport {
        id: id.to_string(),
        ok,
        status: if ok { "connected" } else { "failed" },
        tools,
        error: error.filter(|error| !error.is_empty()),
        probed_at: now_millis(),
    }
}

fn initialize_request() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "crewvela", "version": env!("CARGO_PKG_VERSION") }
        }
    })
}

fn tools_request() -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    })
}

fn initialized_notification() -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    })
}

fn extract_tools(result: &Value) -> Vec<McpToolInfo> {
    result
        .get("tools")
        .and_then(Value::as_array)
        .map(|tools| {
            tools
                .iter()
                .filter_map(|tool| {
                    let name = tool.get("name")?.as_str()?.to_string();
                    let description = tool
                        .get("description")
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    Some(McpToolInfo { name, description })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// 从一行 stdio/http 响应文本里解析 JSON-RPC 结果；流式 http 会把 JSON 包在
/// `data: {...}` SSE 帧里，两种形态都兼容。
fn parse_rpc_payload(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        return Some(value);
    }
    trimmed
        .strip_prefix("data:")
        .and_then(|payload| serde_json::from_str::<Value>(payload.trim()).ok())
}

fn rpc_error_message(value: &Value) -> Option<String> {
    value
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// stdio：spawn 进程，newline-delimited JSON-RPC 走 initialize + tools/list。
fn probe_stdio(server: &McpServerConfig) -> McpProbeReport {
    let command = server.command.clone().unwrap_or_default();
    let mut command_builder = Command::new(&command);
    command_builder
        .args(&server.args)
        .envs(&server.env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command_builder.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = match command_builder.spawn() {
        Ok(child) => child,
        Err(error) => {
            return probe_report(
                &server.id,
                false,
                Vec::new(),
                Some(format!("启动 {command} 失败：{error}")),
            );
        }
    };
    let stdout = child
        .stdout
        .take()
        .expect("stdout 已设为 piped");
    let (sender, receiver) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(line) => {
                    if sender.send(line).is_err() {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
    });

    let mut stdin = child.stdin.take();
    let write_error = |error: std::io::Error| Some(format!("写入 MCP 进程失败：{error}"));
    let mut failure: Option<String> = None;
    if let Some(stdin) = stdin.as_mut() {
        if let Err(error) = writeln!(stdin, "{}", initialize_request()).and_then(|_| stdin.flush()) {
            failure = write_error(error);
        }
    }

    let mut tools: Option<Vec<McpToolInfo>> = None;
    let mut initialize_ok = false;
    let deadline = Instant::now() + STDIO_PROBE_TIMEOUT;
    if failure.is_none() {
        loop {
            let timeout = deadline.saturating_duration_since(Instant::now());
            if timeout.is_zero() { failure = Some("连接超时（45 秒内未完成握手）".into()); break; }
            let line = match receiver.recv_timeout(timeout.min(Duration::from_millis(500))) {
                Ok(line) => line,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    failure = Some("MCP 进程在完成握手前关闭了输出".into()); break;
                }
            };
            let Some(payload) = parse_rpc_payload(&line) else { continue; };
            if let Some(message) = rpc_error_message(&payload) { failure = Some(message); break; }
            match payload.get("id").and_then(Value::as_i64) {
                Some(1) if payload.get("result").is_some() => {
                    initialize_ok = true;
                    if let Some(stdin) = stdin.as_mut() {
                        for payload in [initialized_notification(), tools_request()] {
                            if let Err(error) = writeln!(stdin, "{payload}").and_then(|_| stdin.flush()) {
                                failure = write_error(error); break;
                            }
                        }
                    }
                    if failure.is_some() { break; }
                }
                Some(2) if initialize_ok && payload.get("result").is_some() => {
                    tools = Some(extract_tools(&payload["result"])); break;
                }
                Some(1 | 2) => { failure = Some("MCP 握手响应顺序无效或缺少 result".into()); break; }
                _ => {}
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    drop(stdin);
    if failure.is_none() && initialize_ok && tools.is_some() {
        probe_report(&server.id, true, tools.unwrap_or_default(), None)
    } else {
        probe_report(&server.id, false, Vec::new(), failure)
    }
}

/// http：POST JSON-RPC initialize + tools/list；sse：GET 探活（SSE 需要长连接
/// 才能枚举工具，这里只确认端点可达，工具数显示为 0）。
pub(crate) async fn probe_remote(server: &McpServerConfig) -> McpProbeReport {
    let client = match crate::general_settings::configure_http_client(reqwest::Client::builder())
        .timeout(REMOTE_PROBE_TIMEOUT)
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return probe_report(&server.id, false, Vec::new(), Some(format!("创建 HTTP 客户端失败：{error}")));
        }
    };
    let url = server.url.clone().unwrap_or_default();
    let mut request = client.get(&url);
    for (key, value) in &server.headers {
        request = request.header(key, value);
    }
    if server.transport == McpTransport::Sse {
        return match request.send().await {
            Ok(response) if response.status().is_success() => {
                probe_report(&server.id, true, Vec::new(), None)
            }
            Ok(response) => probe_report(
                &server.id,
                false,
                Vec::new(),
                Some(format!("HTTP {}", response.status().as_u16())),
            ),
            Err(error) => probe_report(&server.id, false, Vec::new(), Some(error.to_string())),
        };
    }

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert(reqwest::header::ACCEPT, "application/json, text/event-stream".parse().expect("静态 HeaderValue 必须合法"));
    headers.insert(
        reqwest::header::CONTENT_TYPE,
        "application/json".parse().expect("静态 HeaderValue 必须合法"),
    );
    for (key, value) in &server.headers {
        if let (Ok(name), Ok(value)) = (
            reqwest::header::HeaderName::try_from(key.as_str()),
            reqwest::header::HeaderValue::try_from(value.as_str()),
        ) {
            headers.insert(name, value);
        }
    }
    match probe_http(&client, &url, headers).await {
        Ok(tools) => probe_report(&server.id, true, tools, None),
        Err(error) => probe_report(&server.id, false, Vec::new(), Some(format!("{error:#}"))),
    }
}

fn response_payload(bytes: &[u8], expected_id: i64) -> Option<Value> {
    let matches = |value: &Value| value.get("id").and_then(Value::as_i64) == Some(expected_id);
    if let Ok(value) = serde_json::from_slice::<Value>(bytes) {
        if matches(&value) { return Some(value); }
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let mut data = String::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            if let Ok(value) = serde_json::from_str::<Value>(&data) {
                if matches(&value) { return Some(value); }
            }
            data.clear();
        } else if let Some(value) = line.strip_prefix("data:") {
            if !data.is_empty() { data.push('\n'); }
            data.push_str(value.trim_start());
        }
    }
    serde_json::from_str::<Value>(&data).ok().filter(matches)
}

async fn read_rpc_response(mut response: reqwest::Response, expected_id: i64) -> Result<Value> {
    if !response.status().is_success() { bail!("MCP 返回 HTTP {}", response.status().as_u16()); }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        if bytes.len().saturating_add(chunk.len()) > 1024 * 1024 { bail!("MCP 响应超过 1 MB 上限"); }
        bytes.extend_from_slice(&chunk);
        if let Some(payload) = response_payload(&bytes, expected_id) {
            if let Some(message) = rpc_error_message(&payload) { bail!("{message}"); }
            if payload.get("result").is_none() { bail!("MCP 响应缺少 result"); }
            return Ok(payload);
        }
    }
    bail!("MCP 响应缺少预期 JSON-RPC 结果（id {expected_id}）")
}

async fn probe_http(client: &reqwest::Client, url: &str, mut headers: reqwest::header::HeaderMap) -> Result<Vec<McpToolInfo>> {
    let response = client.post(url).headers(headers.clone()).json(&initialize_request()).send().await?;
    let session = response.headers().get("mcp-session-id").cloned();
    let initialized = read_rpc_response(response, 1).await.context("initialize 失败")?;
    if let Some(session) = &session { headers.insert("mcp-session-id", session.clone()); }
    if let Some(protocol) = initialized.pointer("/result/protocolVersion").and_then(Value::as_str) {
        headers.insert("mcp-protocol-version", protocol.parse().context("MCP 协议版本 Header 无效")?);
    }
    let result = async {
        let notification = client.post(url).headers(headers.clone()).json(&initialized_notification()).send().await?;
        if !notification.status().is_success() { bail!("initialized 通知返回 HTTP {}", notification.status().as_u16()); }
        let response = client.post(url).headers(headers.clone()).json(&tools_request()).send().await?;
        let payload = read_rpc_response(response, 2).await.context("tools/list 失败")?;
        Ok(extract_tools(&payload["result"]))
    }.await;
    if session.is_some() {
        // This is the session created by this probe; never terminate an external session.
        let _ = client.delete(url).headers(headers).timeout(Duration::from_secs(2)).send().await;
    }
    result
}

/// 探测全部启用的 server：stdio 走阻塞线程池，远程并发走异步。
pub async fn probe_enabled() -> Vec<McpProbeReport> {
    let Ok(servers) = load() else {
        return Vec::new();
    };
    let mut reports = Vec::new();
    let mut stdio_handles = Vec::new();
    for server in servers.into_iter().filter(|server| server.enabled) {
        if server.transport == McpTransport::Stdio {
            stdio_handles.push(tauri::async_runtime::spawn_blocking(move || {
                probe_stdio(&server)
            }));
        } else {
            reports.push(probe_remote(&server).await);
        }
    }
    for handle in stdio_handles {
        match handle.await {
            Ok(report) => reports.push(report),
            Err(error) => reports.push(probe_report(
                "unknown",
                false,
                Vec::new(),
                Some(format!("探测线程异常：{error}")),
            )),
        }
    }
    reports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stdio_probe_requires_successful_ordered_handshake() {
        for failure in [true, false] {
            let mut server_input = input("probe-fixture", McpTransport::Stdio);
            server_input.command = Some(which::which("node").unwrap().display().to_string());
            let mut server = validate(&server_input, &[]).unwrap();
            server.args = vec!["-e".into(), format!(r#"const rl=require('node:readline').createInterface({{input:process.stdin}});
let initialized=false;
rl.on('line',line=>{{const q=JSON.parse(line);const out=v=>console.log(JSON.stringify(v));
 if(q.method==='initialize') out({{jsonrpc:'2.0',id:q.id,...({failure}?{{error:{{code:-1,message:'fixture rejected initialize'}}}}:{{result:{{protocolVersion:'2024-11-05'}}}})}});
 else if(q.method==='notifications/initialized') initialized=true;
 else if(q.method==='tools/list') out(initialized?{{jsonrpc:'2.0',id:q.id,result:{{tools:[{{name:'fixture-tool'}}]}}}}:{{jsonrpc:'2.0',id:q.id,error:{{message:'initialize notification required'}}}});
}});"#)];
            let report = probe_stdio(&server);
            assert_eq!(report.ok, !failure, "{report:?}");
            if failure { assert!(report.error.unwrap().contains("fixture rejected initialize")); }
            else { assert_eq!(report.tools[0].name, "fixture-tool"); }
        }
    }

    fn read_http_fixture_request(stream: &std::net::TcpStream) -> (String, BTreeMap<String, String>, Vec<u8>) {
        use std::io::Read;
        // Only accept polling is nonblocking; request framing must wait for
        // every header/body fragment on Windows as well as on Unix.
        stream.set_nonblocking(false).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request = String::new();
        assert!(reader.read_line(&mut request).unwrap() > 0, "missing fixture request line");
        let mut headers = BTreeMap::new();
        loop {
            let mut line = String::new();
            assert!(reader.read_line(&mut line).unwrap() > 0, "truncated fixture request headers");
            if line == "\r\n" { break; }
            if let Some((name, value)) = line.split_once(':') { headers.insert(name.to_ascii_lowercase(), value.trim().to_string()); }
        }
        let length = headers.get("content-length").and_then(|value| value.parse::<usize>().ok()).unwrap_or(0);
        let mut body = vec![0; length]; reader.read_exact(&mut body).unwrap();
        (request, headers, body)
    }

    #[test]
    fn http_probe_fixture_reads_fragmented_headers_and_body() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted, connected) = std::sync::mpsc::channel();
        let server = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(2);
            let socket = loop {
                match listener.accept() {
                    Ok((socket, _)) => break socket,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::yield_now(),
                    Err(error) => panic!("HTTP fixture accept failed: {error}"),
                }
            };
            accepted.send(()).unwrap();
            read_http_fixture_request(&socket)
        });
        let mut client = std::net::TcpStream::connect(address).unwrap();
        connected.recv_timeout(Duration::from_secs(2)).unwrap();
        client.write_all(b"POST /mcp HTTP/1.1\r\nContent-Length: ").unwrap();
        std::thread::sleep(Duration::from_millis(100));
        let headers_sent = client.write_all(b"2\r\nMcp-Session-Id: fragmented-session\r\n\r\n{");
        std::thread::sleep(Duration::from_millis(100));
        let body_sent = client.write_all(b"}");
        let (request, headers, body) = server.join().unwrap();
        headers_sent.unwrap();
        body_sent.unwrap();
        assert_eq!(request, "POST /mcp HTTP/1.1\r\n");
        assert_eq!(headers.get("mcp-session-id").map(String::as_str), Some("fragmented-session"));
        assert_eq!(body, b"{}");
    }

    #[test]
    fn http_probe_keeps_session_and_finishes_on_a_live_sse_frame() {
        use std::io::Read;
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        listener.set_nonblocking(true).unwrap();
        let observed_notification = Arc::new(AtomicBool::new(false));
        let notification = Arc::clone(&observed_notification);
        let fixture = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(8);
            let mut handlers = Vec::new();
            while handlers.len() < 4 && Instant::now() < deadline {
                let (mut stream, _) = match listener.accept() {
                    Ok(value) => value,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => { std::thread::sleep(Duration::from_millis(5)); continue; }
                    Err(error) => panic!("{error}"),
                };
                let notification = Arc::clone(&notification);
                handlers.push(std::thread::spawn(move || {
                    let (request, headers, body) = read_http_fixture_request(&stream);
                    let delete = request.starts_with("DELETE ");
                    let payload: Value = if body.is_empty() { Value::Null } else { serde_json::from_slice(&body).unwrap() };
                    let method = payload["method"].as_str().unwrap_or("");
                    if method != "initialize" { assert_eq!(headers.get("mcp-session-id").map(String::as_str), Some("probe-owned-session")); }
                    let (status, extra, response) = if method == "initialize" {
                        ("200 OK", "Mcp-Session-Id: probe-owned-session\r\n", r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05"}}"#)
                    } else if method == "notifications/initialized" {
                        notification.store(true, Ordering::Release);
                        ("202 Accepted", "", "")
                    } else if method == "tools/list" {
                        assert!(notification.load(Ordering::Acquire));
                        assert_eq!(headers.get("mcp-protocol-version").map(String::as_str), Some("2024-11-05"));
                        let event = "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[{\"name\":\"stream-tool\"}]}}\n\n";
                        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: 1000000\r\n\r\n{event}").unwrap();
                        stream.flush().unwrap();
                        // Deliberately do not close the SSE body after its first result.
                        let _ = stream.read(&mut [0]);
                        return;
                    } else {
                        assert!(delete);
                        ("204 No Content", "", "")
                    };
                    write!(stream, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n{extra}\r\n{response}", response.len()).unwrap();
                }));
            }
            assert_eq!(handlers.len(), 4, "initialize, initialized, tools/list, and own-session DELETE are required");
            for handler in handlers { handler.join().unwrap(); }
        });
        let mut server_input = input("http-fixture", McpTransport::Http);
        server_input.url = Some(format!("http://{address}/mcp"));
        let server = validate(&server_input, &[]).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let report = runtime.block_on(async { tokio::time::timeout(Duration::from_secs(4), probe_remote(&server)).await.unwrap() });
        assert!(report.ok, "{report:?}"); assert_eq!(report.tools[0].name, "stream-tool");
        assert!(observed_notification.load(Ordering::Acquire));
        fixture.join().unwrap();
    }

    #[test]
    fn concurrent_plugin_registry_writes_preserve_every_entry() {
        let path = tmp_path("concurrent-writes");
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let handles: Vec<_> = (0..8).map(|index| {
            let path = path.clone(); let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                let servers = serde_json::from_value::<serde_json::Map<String, Value>>(json!({"tool":{"command":"node"}})).unwrap();
                import_plugin_servers_at(&path, &format!("key-{index}"), &format!("plugin-{index}"), &servers).unwrap();
            })
        }).collect();
        for handle in handles { handle.join().unwrap(); }
        assert_eq!(load_at(&path).unwrap().len(), 8);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn mcp_arguments_preserve_empty_and_whitespace_values() {
        let mut value = input("literal-args", McpTransport::Stdio);
        value.command = Some("node".into());
        value.args = vec!["".into(), "  literal value  ".into(), " ".into()];
        assert_eq!(validate(&value, &[]).unwrap().args, value.args);
    }

    #[test]
    fn response_reader_handles_framed_multiline_sse() {
        let response = b"event: message\ndata: {\n data: ignored\ndata: \"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}\ndata: }\n\n";
        assert!(response_payload(response, 2).is_some());
    }

    use std::collections::HashSet;

    fn tmp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "crewvela-mcp-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ))
    }

    fn input(name: &str, transport: McpTransport) -> McpUpsertInput {
        McpUpsertInput {
            id: None,
            name: name.into(),
            transport,
            command: None,
            args: Vec::new(),
            env: BTreeMap::new(),
            url: None,
            headers: BTreeMap::new(),
            enabled: true,
            source_plugin: None,
        }
    }

    #[test]
    fn renamed_launcher_refreshes_only_owned_computer_control_commands() {
        let path = tmp_path("renamed-launcher");
        let mut request = input("computer-control", McpTransport::Stdio);
        request.command = Some(format!("C:/old/{}.exe", crate::brand::LEGACY_NAME));
        request.args = vec!["--computer-control-mcp".into(), "C:/runtime with spaces/runtime.ps1".into()];
        request.source_plugin = Some(crate::computer_control::BUILTIN_SOURCE_KEY.into());
        request.enabled = false;
        let owned = validate(&request, &[]).unwrap();
        let mut custom = owned.clone(); custom.source_plugin = None;
        let mut wrapper = owned.clone(); wrapper.command = Some("custom-wrapper.exe".into());
        let mut other_action = owned.clone(); other_action.args[0] = "--other-action".into();
        let original = vec![owned, custom, wrapper, other_action];
        save_at(&path, &original).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let mut loaded = load_at(&path).unwrap();
        refresh_owned_launcher_commands(&mut loaded, Path::new("C:/portable/CrewVela.exe"));
        let mut expected = original.clone();
        expected[0].command = Some("C:/portable/CrewVela.exe".into());
        assert_eq!(serde_json::to_value(&loaded).unwrap(), serde_json::to_value(expected).unwrap());
        assert!(!loaded[0].enabled, "renaming must not enable a disabled service");
        assert_eq!(std::fs::read(&path).unwrap(), bytes, "runtime path refresh must not rewrite user configuration");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn upsert_validates_and_roundtrips_through_disk() {
        let path = tmp_path("roundtrip");
        let mut server = input("filesystem", McpTransport::Stdio);
        server.command = Some("npx".into());
        server.args = vec!("-y".into(), "@modelcontextprotocol/server-filesystem".into(), "/tmp".into());
        let saved = {
            let mut servers = Vec::new();
            let validated = validate(&server, &servers).unwrap();
            servers.push(validated.clone());
            save_at(&path, &servers).unwrap();
            validated
        };
        let loaded = load_at(&path).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "filesystem");
        assert_eq!(loaded[0].args, saved.args);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn stdio_requires_command_and_remote_requires_url() {
        let mut server = input("no-command", McpTransport::Stdio);
        assert!(validate(&server, &[]).is_err());
        server.command = Some("npx".into());
        server.url = Some("http://x".into());
        assert!(validate(&server, &[]).is_err());

        let mut remote = input("no-url", McpTransport::Http);
        assert!(validate(&remote, &[]).is_err());
        remote.url = Some("ftp://bad".into());
        assert!(validate(&remote, &[]).is_err());
        remote.url = Some("https://example.com/mcp".into());
        remote.command = Some("npx".into());
        assert!(validate(&remote, &[]).is_err());
    }

    #[test]
    fn duplicate_names_are_rejected_across_servers() {
        let first = {
            let mut server = input("fs", McpTransport::Stdio);
            server.command = Some("npx".into());
            validate(&server, &[]).unwrap()
        };
        let mut second = input("fs", McpTransport::Http);
        second.url = Some("https://example.com".into());
        assert!(validate(&second, &[first]).is_err());
    }

    #[test]
    fn claude_settings_entry_matches_transport_shape() {
        let mut stdio = input("fs", McpTransport::Stdio);
        stdio.command = Some("npx".into());
        stdio.args = vec!("-y".into(), "server".into());
        stdio.env.insert("API_KEY".into(), "secret".into());
        let entry = claude_settings_entry(&validate(&stdio, &[]).unwrap());
        assert_eq!(entry["command"], "npx");
        assert_eq!(entry["args"][0], "-y");
        assert_eq!(entry["env"]["API_KEY"], "secret");
        assert!(entry.get("type").is_none());

        let mut http = input("kb", McpTransport::Http);
        http.url = Some("https://example.com/mcp".into());
        http.headers.insert("Authorization".into(), "Bearer x".into());
        let entry = claude_settings_entry(&validate(&http, &[]).unwrap());
        assert_eq!(entry["type"], "http");
        assert_eq!(entry["url"], "https://example.com/mcp");
        assert_eq!(entry["headers"]["Authorization"], "Bearer x");

        let mut sse = input("legacy", McpTransport::Sse);
        sse.url = Some("https://example.com/sse".into());
        let entry = claude_settings_entry(&validate(&sse, &[]).unwrap());
        assert_eq!(entry["type"], "sse");
        assert!(entry.get("headers").is_none());
    }

    #[test]
    fn enabled_claude_map_only_contains_enabled_servers() {
        let mut servers = Vec::new();
        let mut enabled = input("on", McpTransport::Http);
        enabled.url = Some("https://example.com/mcp".into());
        servers.push(validate(&enabled, &[]).unwrap());
        let mut disabled = input("off", McpTransport::Http);
        disabled.url = Some("https://example.com/other".into());
        disabled.enabled = false;
        servers.push(validate(&disabled, &[]).unwrap());

        let path = tmp_path("map");
        save_at(&path, &servers).unwrap();
        let map = {
            // enabled_claude_map 读全局路径，这里直接基于 load_at 验证同构逻辑。
            let mut map = serde_json::Map::new();
            for server in load_at(&path).unwrap() {
                if server.enabled {
                    map.insert(server.name.clone(), claude_settings_entry(&server));
                }
            }
            map
        };
        assert_eq!(map.len(), 1);
        assert!(map.contains_key("on"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn codex_overrides_take_enabled_stdio_only_and_escape_windows_paths() {
        let mut servers = Vec::new();
        let mut browser = input("browser-control", McpTransport::Stdio);
        browser.command = Some(r"C:\Tools\node.exe".into());
        browser.args = vec![r"C:\mcp\cli.js".into(), "--isolated".into()];
        browser.env.insert("API_KEY".into(), "b c".into());
        servers.push(validate(&browser, &[]).unwrap());
        let mut off = input("off", McpTransport::Stdio);
        off.command = Some("node".into());
        off.enabled = false;
        servers.push(validate(&off, &[]).unwrap());
        let mut remote = input("remote", McpTransport::Http);
        remote.url = Some("https://example.com/mcp".into());
        servers.push(validate(&remote, &[]).unwrap());

        let path = tmp_path("codex");
        save_at(&path, &servers).unwrap();
        assert_eq!(
            codex_cli_mcp_overrides_at(&path),
            vec![
                r#"mcp_servers."browser-control".command="C:\\Tools\\node.exe""#.to_string(),
                r#"mcp_servers."browser-control".args=["C:\\mcp\\cli.js", "--isolated"]"#.to_string(),
                r#"mcp_servers."browser-control".env={ "API_KEY" = "b c" }"#.to_string(),
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rpc_payload_parses_plain_json_and_sse_frames() {
        let plain = parse_rpc_payload(r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[{"name":"search"}]}}"#);
        assert_eq!(
            plain.as_ref().and_then(|p| p.pointer("/result/tools/0/name")).and_then(Value::as_str),
            Some("search")
        );
        let framed = parse_rpc_payload("data: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"tools\":[]}}");
        assert!(framed.is_some());
        assert!(parse_rpc_payload("event: message").is_none());
    }

    #[test]
    fn extract_tools_skips_entries_without_names() {
        let payload = json!({
            "tools": [
                { "name": "search", "description": "全文检索" },
                { "description": "缺名字，跳过" },
                { "name": "read" }
            ]
        });
        let tools = extract_tools(&payload);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "search");
        assert_eq!(tools[0].description.as_deref(), Some("全文检索"));
        assert_eq!(tools[1].description, None);
    }

    #[test]
    fn probe_stdio_reports_spawn_failure() {
        let mut server = input("ghost", McpTransport::Stdio);
        server.command = Some("definitely-missing-binary-crewvela".into());
        let report = probe_stdio(&validate(&server, &[]).unwrap());
        assert!(!report.ok);
        assert!(report.error.unwrap().contains("启动"));
    }

    #[test]
    fn acp_entry_converts_http_and_stdio_sse_stays_out() {
        let mut http = input("kb", McpTransport::Http);
        http.url = Some("https://example.com/mcp".into());
        http.headers.insert("Authorization".into(), "Bearer x".into());
        let entry = acp_server_entry(&validate(&http, &[]).unwrap()).expect("http 应转换");
        assert_eq!(entry["type"], "http");
        assert_eq!(entry["name"], "kb");
        assert_eq!(entry["url"], "https://example.com/mcp");
        assert_eq!(entry["headers"][0]["name"], "Authorization");
        assert_eq!(entry["headers"][0]["value"], "Bearer x");

        // stdio：{name, command, args, env}，env 转 [{name,value}] 数组形态。
        let mut stdio = input("fs", McpTransport::Stdio);
        stdio.command = Some("npx".into());
        stdio.args = vec!("-y".into(), "mcp-fetch".into());
        stdio.env.insert("API_KEY".into(), "secret".into());
        let entry = acp_server_entry(&validate(&stdio, &[]).unwrap()).expect("stdio 应转换");
        assert!(entry.get("type").is_none(), "stdio 形态不带 type 字段");
        assert_eq!(entry["name"], "fs");
        assert_eq!(entry["command"], "npx");
        assert_eq!(entry["args"][0], "-y");
        assert_eq!(entry["env"][0]["name"], "API_KEY");
        assert_eq!(entry["env"][0]["value"], "secret");

        // SSE：ACP 只协商 http 能力，继续走 claude 链路。
        let mut sse = input("legacy", McpTransport::Sse);
        sse.url = Some("https://example.com/sse".into());
        assert!(acp_server_entry(&validate(&sse, &[]).unwrap()).is_none());
    }

    #[test]
    fn stdio_entry_always_carries_args_and_env() {
        // ACP SDK 1.3 的 McpServerStdio schema 把 args/env 列为 required，缺字段
        // 会被 x-deserialize-skip-invalid-items 静默丢弃——内置条目（如浏览器控制）
        // env 恒空，必须显式输出 [] 才能到达适配器。
        let mut stdio = input("browser-control", McpTransport::Stdio);
        stdio.command = Some("node".into());
        stdio.args = vec!["bridge.mjs".into()];
        let entry = acp_server_entry(&validate(&stdio, &[]).unwrap()).expect("stdio 应转换");
        assert_eq!(entry["args"].as_array().map(Vec::len), Some(1));
        assert_eq!(entry["env"].as_array().map(Vec::len), Some(0), "env 空也必须是显式空数组");
    }

    #[test]
    fn plugin_servers_roundtrip_with_prefix_and_inplace_reinstall() {
        let path = tmp_path("plugin-mcp");
        let servers: serde_json::Map<String, Value> = serde_json::json!({
            "fetch": { "command": "npx", "args": ["-y", "mcp-fetch"] },
            "kb": { "type": "http", "url": "https://example.com/mcp", "headers": { "Authorization": "Bearer t" } }
        })
        .as_object()
        .unwrap()
        .clone();

        let first = import_plugin_servers_at(&path, "demo@market", "demo", &servers).unwrap();
        assert_eq!(first.len(), 2);
        assert!(first.iter().all(|server| server.enabled));
        assert!(first
            .iter()
            .all(|server| server.source_plugin.as_deref() == Some("demo@market")));
        let fetch = first.iter().find(|s| s.name == "demo-fetch").unwrap();
        assert_eq!(fetch.transport, McpTransport::Stdio);
        assert_eq!(fetch.args, vec!["-y".to_string(), "mcp-fetch".to_string()]);
        let kb = first.iter().find(|s| s.name == "demo-kb").unwrap();
        assert_eq!(kb.transport, McpTransport::Http);
        assert_eq!(kb.headers.get("Authorization").map(String::as_str), Some("Bearer t"));

        // 重装：原位更新（保留 id 与用户设置的启停状态），不产生重复条目。
        let mut disabled = first.clone();
        for server in &mut disabled {
            server.enabled = false;
        }
        save_at(&path, &disabled).unwrap();
        let second = import_plugin_servers_at(&path, "demo@market", "demo", &servers).unwrap();
        assert_eq!(second.len(), 2);
        let loaded = load_at(&path).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(
            second.iter().map(|s| s.id.clone()).collect::<HashSet<_>>(),
            first.iter().map(|s| s.id.clone()).collect::<HashSet<_>>()
        );
        assert!(second.iter().all(|s| !s.enabled)); // 用户停用过的保持停用

        // 卸载：只清本插件的，不动用户手配。
        let mut with_user = load_at(&path).unwrap();
        with_user.push(validate(
            &{
                let mut i = input("mine", McpTransport::Http);
                i.url = Some("https://mine.example.com".into());
                i
            },
            &[],
        ).unwrap());
        save_at(&path, &with_user).unwrap();
        assert_eq!(remove_plugin_servers_at(&path, "demo@market").unwrap(), 2);
        let after = load_at(&path).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].name, "mine");
        assert!(after[0].source_plugin.is_none());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn plugin_server_definition_requires_command_or_url() {
        let now = now_millis();
        assert!(server_config_from_json(
            "bad".into(),
            &serde_json::json!({ "args": [] }),
            "p@m",
            now
        )
        .is_err());
        assert!(server_config_from_json(
            "bad-http".into(),
            &serde_json::json!({ "type": "http" }),
            "p@m",
            now
        )
        .is_err());
    }
}
