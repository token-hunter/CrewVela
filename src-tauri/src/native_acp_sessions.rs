use std::io::BufRead;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

use crate::acp::{terminate_acp_process, AcpConnection, AcpPromptResult, ManagedPermissionHandler};
use crate::sessions::{is_standalone_claude_chat, is_standalone_codex_chat, is_standalone_deepseek_harness, is_standalone_kimi_chat, is_standalone_opencode_chat, SessionManager, SessionRecord, SessionStatus};

/// 原生 ACP 聊天会话的运行时分支：dsh 走控制插件 + TCP 流，Claude / Kimi / Codex / OpenCode 只走标准 ACP。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativeChatFlavor {
    DeepSeekHarness,
    ClaudeChat,
    KimiChat,
    CodexChat,
    OpenCodeChat,
}

impl NativeChatFlavor {
    pub fn label(self) -> &'static str {
        match self {
            Self::DeepSeekHarness => "DeepSeek Harness",
            Self::ClaudeChat => "Claude Code Chat",
            Self::KimiChat => "Kimi Code Chat",
            Self::CodexChat => "Codex Chat",
            Self::OpenCodeChat => "OpenCode Chat",
        }
    }

    pub fn event_name(self) -> &'static str {
        match self {
            Self::DeepSeekHarness => "deepseek-harness-update",
            Self::ClaudeChat => "claude-chat-update",
            Self::KimiChat => "kimi-chat-update",
            Self::CodexChat => "codex-chat-update",
            Self::OpenCodeChat => "opencode-chat-update",
        }
    }

    pub fn error_event_name(self) -> &'static str {
        match self {
            Self::DeepSeekHarness => "deepseek-harness-error",
            Self::ClaudeChat => "claude-chat-error",
            Self::KimiChat => "kimi-chat-error",
            Self::CodexChat => "codex-chat-error",
            Self::OpenCodeChat => "opencode-chat-error",
        }
    }

    fn permission_mode(self) -> &'static str {
        match self {
            Self::DeepSeekHarness => "workspace-write",
            Self::ClaudeChat => "default",
            Self::KimiChat => "default",
            Self::CodexChat => "agent",
            // opencode 权限模型是逐工具 ask（映射审批卡片），没有全局 bypass mode。
            Self::OpenCodeChat => "default",
        }
    }

    fn is_standalone(self, record: &SessionRecord) -> bool {
        match self {
            Self::DeepSeekHarness => is_standalone_deepseek_harness(record),
            Self::ClaudeChat => is_standalone_claude_chat(record),
            Self::KimiChat => is_standalone_kimi_chat(record),
            Self::CodexChat => is_standalone_codex_chat(record),
            Self::OpenCodeChat => is_standalone_opencode_chat(record),
        }
    }
}
const PERMISSION_WAIT: Duration = Duration::from_secs(30 * 60);
const PROMPT_MAX_CHARS: usize = 32_000;

// ACP resource links preserve the file identity; the agent opens them using its
// own tools and permission policy. Binary files are never decoded as text here.
fn valid_codex_plugin_id(id: &str) -> bool {
    let parts = id.split('@').collect::<Vec<_>>();
    parts.len() == 2 && parts.iter().all(|part| !part.is_empty() && part.len() <= 160 && part.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
}

fn available_codex_plugin_ids(catalog: &Value) -> HashSet<String> {
    catalog["marketplaces"].as_array().into_iter().flatten()
        .flat_map(|market| market["plugins"].as_array().into_iter().flatten())
        .filter(|plugin| plugin["installed"] == true && plugin["enabled"] == true && plugin["availability"] == "AVAILABLE")
        .filter_map(|plugin| plugin["id"].as_str().filter(|id| valid_codex_plugin_id(id)).map(str::to_owned)).collect()
}

fn normalize_attachments(paths: &[String]) -> Result<Vec<HarnessAttachment>> {
    if paths.len() > 16 { return Err(anyhow!("每条消息最多附加 16 个文件")); }
    let mut files = Vec::new();
    for input in paths {
        let path = fs::canonicalize(input).with_context(|| format!("附件不可访问：{input}"))?;
        if !path.is_file() { return Err(anyhow!("附件必须是文件：{input}")); }
        let path_string = crate::acp::normalize_harness_path(&path)?;
        if files.iter().any(|file: &HarnessAttachment| file.path == path_string) { continue; }
        let uri = tauri::Url::from_file_path(&path_string).map_err(|_| anyhow!("无法转换附件路径：{input}"))?.to_string();
        let name = path.file_name().and_then(|name| name.to_str()).ok_or_else(|| anyhow!("附件名称不是有效 UTF-8"))?.to_string();
        files.push(HarnessAttachment { path: path_string, uri, name });
    }
    Ok(files)
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessAttachment {
    pub path: String,
    pub uri: String,
    pub name: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HarnessChatMessage {
    pub id: String,
    pub role: String,
    pub text: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub reasoning: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stream_status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<HarnessAttachment>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionLocator {
    cwd: String,
    permission_mode: String,
    acp_session_id: Option<String>,
    history: Vec<HarnessChatMessage>,
    config_options: Vec<Value>,
    #[serde(default)]
    commands: Vec<Value>,
    #[serde(default)]
    context_usage: Option<crate::acp::AcpContextUsage>,
    #[serde(default)]
    codex_goal: Option<crate::acp::CodexGoal>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedSessions {
    version: u32,
    sessions: HashMap<String, SessionLocator>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HarnessSessionSnapshot {
    pub session_id: String,
    pub connected: bool,
    pub busy: bool,
    pub history: Vec<HarnessChatMessage>,
    pub config_options: Vec<Value>,
    pub commands: Vec<Value>,
    pub pending_permission: Option<Value>,
    pub tool_calls: Vec<Value>,
    pub sequence: u64,
    pub control_state: Value,
    pub context_usage: Option<crate::acp::AcpContextUsage>,
    pub codex_goal: Option<crate::acp::CodexGoal>,
    pub codex_goal_supported: bool,
    pub codex_goal_action: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HarnessEvent {
    session_id: String,
    sequence: u64,
    kind: String,
    data: Value,
    record: Option<SessionRecord>,
    snapshot: Option<HarnessSessionSnapshot>,
}

struct LiveSession {
    lifecycle: Mutex<()>,
    publication: Mutex<()>,
    connection: Mutex<Option<AcpConnection>>,
    process_id: Arc<AtomicU32>,
    busy: AtomicBool,
    prompt_inflight: AtomicBool,
    stream_authoritative: AtomicBool,
    stream_revision: AtomicU64,
    stream_epoch: AtomicU64,
    stream_failure: Mutex<Option<String>>,
    connected: AtomicBool,
    cancelled: AtomicBool,
    cancel_requested: AtomicBool,
    generation: AtomicU64,
    /// Claude 适配器 session/load 后会整段重放历史通知；本地已持久化历史，
    /// 重放期间丢弃 chunk/tool_call，首次发送后解除。
    replaying: AtomicBool,
    pending_permissions: Mutex<HashMap<String, mpsc::SyncSender<bool>>>,
    active_turn_id: Mutex<Option<String>>,
    pending_delta: Mutex<(String, std::time::Instant)>,
    /// 标准 ACP flavor 的 agent_thought_chunk 增量缓冲；dsh 走 TCP 流恒为空。
    pending_reasoning: Mutex<String>,
    view: Mutex<LiveView>,
    control_handle: Mutex<Option<(crate::harness_control::HarnessControl, String)>>,
    cancel_handle: Mutex<Option<crate::acp::AcpCancelHandle>>,
    goal_handle: Mutex<Option<crate::acp::CodexGoalHandle>>,
    goal_revision: AtomicU64,
    goal_executing: AtomicBool,
    goal_submission: Mutex<()>,
    codex_thread_active: AtomicBool,
    codex_thread_reported: AtomicBool,
}

#[derive(Default)]
struct LiveView {
    sequence: u64,
    pending_permission: Option<Value>,
    tool_calls: HashMap<String, Value>,
    control_state: Value,
    goal_supported: bool,
    goal_action: Option<String>,
}

impl LiveSession {
    fn new() -> Self {
        Self {
            lifecycle: Mutex::new(()),
            publication: Mutex::new(()),
            connection: Mutex::new(None),
            process_id: Arc::new(AtomicU32::new(0)),
            busy: AtomicBool::new(false),
            prompt_inflight: AtomicBool::new(false),
            stream_authoritative: AtomicBool::new(false),
            stream_revision: AtomicU64::new(0),
            stream_epoch: AtomicU64::new(0),
            stream_failure: Mutex::new(None),
            connected: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            cancel_requested: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            replaying: AtomicBool::new(false),
            pending_permissions: Mutex::new(HashMap::new()),
            active_turn_id: Mutex::new(None),
            pending_delta: Mutex::new((String::new(), std::time::Instant::now())),
            pending_reasoning: Mutex::new(String::new()),
            view: Mutex::new(LiveView::default()),
            goal_handle: Mutex::new(None),
            goal_revision: AtomicU64::new(0),
            goal_executing: AtomicBool::new(false),
            goal_submission: Mutex::new(()),
            codex_thread_active: AtomicBool::new(false),
            codex_thread_reported: AtomicBool::new(false),
            control_handle: Mutex::new(None),
            cancel_handle: Mutex::new(None),
        }
    }

    fn ensure_codex_goal_turn(&self) {
        let mut turn = self.active_turn_id.lock().unwrap();
        if turn.is_none() {
            *self.stream_failure.lock().unwrap() = None;
            *turn = Some(uuid::Uuid::new_v4().to_string());
        }
    }
}

#[derive(Clone)]
pub struct NativeAcpSessions {
    flavor: NativeChatFlavor,
    path: PathBuf,
    store: Arc<Mutex<PersistedSessions>>,
    live: Arc<Mutex<HashMap<String, Arc<LiveSession>>>>,
    #[cfg(test)]
    pub(crate) test_peer: Option<PathBuf>,
}

/// Claude Code 原生聊天会话的独立 State 包装，与 dsh 的 NativeAcpSessions 区分类型。
#[derive(Clone)]
pub struct NativeClaudeSessions(pub NativeAcpSessions);

/// Kimi Code 原生聊天会话的独立 State 包装，与 dsh 的 NativeAcpSessions 区分类型。
#[derive(Clone)]
pub struct NativeKimiSessions(pub NativeAcpSessions);

/// Codex 原生聊天会话的独立 State 包装，与 dsh 的 NativeAcpSessions 区分类型。
#[derive(Clone)]
pub struct NativeCodexChatSessions(pub NativeAcpSessions);

/// OpenCode 原生聊天会话的独立 State 包装，与 dsh 的 NativeAcpSessions 区分类型。
#[derive(Clone)]
pub struct NativeOpenCodeChatSessions(pub NativeAcpSessions);

impl NativeAcpSessions {
    pub fn open_at(path: PathBuf) -> Result<Self> {
        Self::open_at_flavor(path, NativeChatFlavor::DeepSeekHarness)
    }

    pub fn open_at_flavor(path: PathBuf, flavor: NativeChatFlavor) -> Result<Self> {
        let persisted = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| format!("解析{}会话历史失败", flavor.label()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => PersistedSessions {
                version: 1,
                ..PersistedSessions::default()
            },
            Err(error) => return Err(error).with_context(|| format!("读取{}会话历史失败", flavor.label())),
        };
        if persisted.version != 1 {
            return Err(anyhow!("不支持的{}历史版本: {}", flavor.label(), persisted.version));
        }
        Ok(Self {
            flavor,
            path,
            store: Arc::new(Mutex::new(persisted)),
            live: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            test_peer: None,
        })
    }

    pub fn register(&self, session_id: &str, cwd: &str) -> Result<()> {
        validate_cc_session_id(session_id)?;
        let cwd = canonical_directory(cwd)?;
        self.mutate_store(|store| {
            if store.sessions.contains_key(session_id) {
                return Err(anyhow!("{} 会话已存在: {session_id}", self.flavor.label()));
            }
            store.sessions.insert(
                session_id.to_string(),
                SessionLocator {
                    cwd,
                    permission_mode: self.flavor.permission_mode().into(),
                    acp_session_id: None,
                    history: Vec::new(),
                    config_options: Vec::new(),
                    commands: Vec::new(),
                    context_usage: None,
                    codex_goal: None,
                },
            );
            Ok(())
        })
    }

    pub fn contains(&self, session_id: &str) -> bool {
        self.store
            .lock()
            .map(|store| store.sessions.contains_key(session_id))
            .unwrap_or(false)
    }

    pub fn snapshot(&self, session_id: &str) -> Result<HarnessSessionSnapshot> {
        let live = self.live_session(session_id)?;
        let _publication = live.publication.lock().map_err(|_| anyhow!("{} 发布锁已损坏", self.flavor.label()))?;
        self.snapshot_unlocked(session_id, &live)
    }

    fn snapshot_unlocked(&self, session_id: &str, live: &LiveSession) -> Result<HarnessSessionSnapshot> {
        let locator = self.locator(session_id)?;
        let view = live.view.lock().map_err(|_| anyhow!("{} 视图锁已损坏", self.flavor.label()))?;
        Ok(HarnessSessionSnapshot {
            session_id: session_id.to_string(),
            connected: live.connected.load(Ordering::Acquire),
            busy: live.busy.load(Ordering::Acquire),
            history: locator.history,
            config_options: locator.config_options,
            commands: locator.commands,
            pending_permission: view.pending_permission.clone(),
            tool_calls: view.tool_calls.values().cloned().collect(),
            sequence: view.sequence,
            control_state: view.control_state.clone(),
            context_usage: locator.context_usage,
            codex_goal: locator.codex_goal,
            codex_goal_supported: view.goal_supported,
            codex_goal_action: view.goal_action.clone(),
        })
    }

    fn ensure_record(&self, record: &SessionRecord) -> Result<()> {
        validate_cc_session_id(&record.id)?;
        if !self.flavor.is_standalone(record) {
            return Err(anyhow!("会话不是独立{}ACP 会话", self.flavor.label()));
        }
        Ok(())
    }

    pub fn start<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        sessions: &SessionManager,
        record: &SessionRecord,
    ) -> Result<HarnessSessionSnapshot> {
        let _admission = app.try_state::<crate::runtime_admission::RuntimeAdmission>()
            .map(|gate| gate.admit()).transpose()?;
        self.ensure_record(record)?;
        let live = self.live_session(&record.id)?;
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("{} 生命周期锁已损坏", self.flavor.label()))?;
        let generation = live.generation.load(Ordering::Acquire);
        let result = self.start_connection(app.clone(), sessions, record, &live);
        if let Err(error) = &result {
            if live.generation.load(Ordering::Acquire) == generation
                && sessions.get(&record.id).is_some_and(|current| current.archived_at.is_none())
            {
                if let Ok(updated) = sessions.mark_native_runtime_status(&record.id, SessionStatus::Error, None, Some(error.to_string())) {
                    self.emit(&app, &live, &record.id, "error", json!({"message":error.to_string()}), Some(updated));
                }
            }
        }
        result
    }

    fn start_connection<R: tauri::Runtime>(
        &self, app: AppHandle<R>, sessions: &SessionManager,
        record: &SessionRecord, live: &Arc<LiveSession>,
    ) -> Result<HarnessSessionSnapshot> {
        let locator = self.locator(&record.id)?;
        let cwd = canonical_directory(&record.cwd)?;
        if cwd != canonical_directory(&locator.cwd)? || locator.permission_mode != self.flavor.permission_mode() {
            return Err(anyhow!("{}会话目录或权限与持久化恢复定位不匹配", self.flavor.label()));
        }
        #[cfg(target_os = "windows")]
        if self.flavor == NativeChatFlavor::DeepSeekHarness
            && locator.acp_session_id.is_some()
            && locator.cwd.starts_with(r"\\?\")
            && locator.cwd != cwd
        {
            // Harness resume retains its upstream session.header.cwd instead
            // of replacing it with the ACP request cwd. Preserve that session.
            return Err(anyhow!("旧 Harness 会话保存了不兼容的 Windows 路径，请新建会话；原消息历史保留"));
        }
        if record.archived_at.is_some() || sessions.get(&record.id).is_none_or(|current| current.archived_at.is_some()) {
            return Err(anyhow!("归档或已删除的会话不能启动"));
        }
        if live.busy.load(Ordering::Acquire) {
            return self.snapshot(&record.id);
        }
        let mut connection_slot = live
            .connection
            .lock()
            .map_err(|_| anyhow!("{} 连接锁已损坏", self.flavor.label()))?;
        if connection_slot.is_some() {
            drop(connection_slot);
            return self.snapshot(&record.id);
        }
        let generation = live.generation.load(Ordering::Acquire);
        live.cancelled.store(false, Ordering::Release);
        *live.stream_failure.lock().unwrap() = None;
        let permission_handler = self.permission_handler(
            app.clone(),
            sessions.clone(),
            record.id.clone(),
            Arc::clone(&live),
            generation,
        );
        let resume_id = locator.acp_session_id.as_deref();
        let resumed_expected = locator.acp_session_id.is_some();
        let use_policy_command = self.flavor == NativeChatFlavor::OpenCodeChat && crate::session_switch::is_handoff_target(&record.id)?;
        #[cfg(test)] let use_policy_command = use_policy_command && self.test_peer.is_none();
        let command_override = if use_policy_command {
            let mut command = crate::acp::opencode_acp_command()?;
            crate::session_switch_permissions::opencode(&mut command, &record.id, false)?;
            Some(command)
        } else { None };
        #[cfg(test)]
        let command_override = self.test_peer.as_ref().map(|path| {
            let mut command = std::process::Command::new("node");
            command.arg(path);
            command
        }).or(command_override);
        // 原生聊天统一注入启用的 stdio MCP（浏览器控制等）；http/sse 条目
        // 需要 agent 侧能力协商，暂不进入原生聊天。读取失败不阻塞会话启动。
        let mcp_servers = crate::mcp::enabled_native_chat_servers().unwrap_or_else(|error| {
            eprintln!("[crewvela] 读取原生聊天 MCP 配置失败：{error:#}");
            Vec::new()
        });
        // 更新桥接脚本即可；只有实际浏览器工具调用才激活面板。
        if mcp_servers
            .iter()
            .any(|server| server.get("name").and_then(serde_json::Value::as_str) == Some("browser-control"))
        {
            crate::browser_control::refresh_bridge();
        }
        let mut connection = if self.flavor == NativeChatFlavor::ClaudeChat {
            crate::multi_agent::AgentRuntimeKind::ClaudeChat.ensure_available()?;
            let connection = AcpConnection::spawn_native_claude_chat(
                &cwd,
                resume_id,
                mcp_servers.clone(),
                permission_handler,
                Arc::clone(&live.process_id),
                command_override,
            )?;
            // Claude 适配器在 session/load 失败时会静默回退 session/new；
            // 本地持久化历史会与新上游会话错位，此时拒绝启动以保护历史。
            if resumed_expected && !connection.resumed() {
                connection.terminate();
                return Err(anyhow!("Claude Code Chat 会话恢复失败：适配器回退到了新会话，请新建会话；原消息历史保留"));
            }
            connection
        } else if self.flavor == NativeChatFlavor::KimiChat {
            // kimi_chat 原生聊天持独立启停开关；探测/升级仍复用 kimi_code CLI。
            crate::multi_agent::AgentRuntimeKind::KimiChat.ensure_available()?;
            let connection = AcpConnection::spawn_native_kimi_chat(
                &cwd,
                resume_id,
                mcp_servers.clone(),
                permission_handler,
                Arc::clone(&live.process_id),
                command_override,
            )
            .map_err(kimi_chat_spawn_error)?;
            // kimi 的 session/load 失败会直接报错（不回退 session/new）；
            // 若上游仍回退到新会话，则与本地持久化历史错位，拒绝启动以保护历史。
            if resumed_expected && !connection.resumed() {
                connection.terminate();
                return Err(anyhow!("Kimi Code Chat 会话恢复失败：上游回退到了新会话，请新建会话；原消息历史保留"));
            }
            connection
        } else if self.flavor == NativeChatFlavor::CodexChat {
            // codex_chat 原生聊天持独立启停开关；探测/升级仍复用 codex CLI。
            crate::multi_agent::AgentRuntimeKind::CodexChat.ensure_available()?;
            let connection = AcpConnection::spawn_native_codex_chat(
                &cwd,
                resume_id,
                mcp_servers.clone(),
                permission_handler,
                Arc::clone(&live.process_id),
                command_override,
            )?;
            // codex-acp 的 session/load 失败时同样会回退 session/new；
            // 与本地持久化历史错位时拒绝启动以保护历史。
            if resumed_expected && !connection.resumed() {
                connection.terminate();
                return Err(anyhow!("Codex Chat 会话恢复失败：适配器回退到了新会话，请新建会话；原消息历史保留"));
            }
            connection
        } else if self.flavor == NativeChatFlavor::OpenCodeChat {
            // opencode_chat 原生聊天持独立启停开关；探测/升级仍复用 opencode CLI。
            crate::multi_agent::AgentRuntimeKind::OpenCodeChat.ensure_available()?;
            let connection = AcpConnection::spawn_native_opencode_chat(
                &cwd,
                resume_id,
                mcp_servers.clone(),
                permission_handler,
                Arc::clone(&live.process_id),
                command_override,
            )
            .map_err(opencode_chat_spawn_error)?;
            // opencode 的 session/load 同样在响应前回放历史；若 load 失败回退到新会话，
            // 与本地持久化历史错位时拒绝启动以保护历史。
            if resumed_expected && !connection.resumed() {
                connection.terminate();
                return Err(anyhow!("OpenCode Chat 会话恢复失败：上游回退到了新会话，请新建会话；原消息历史保留"));
            }
            connection
        } else {
            AcpConnection::spawn_native_deepseek_harness(
                &cwd,
                resume_id,
                mcp_servers,
                permission_handler,
                Arc::clone(&live.process_id),
                command_override,
            )?
        };
        if live.generation.load(Ordering::Acquire) != generation {
            connection.terminate();
            return Err(anyhow!("{} 会话启动已取消", self.flavor.label()));
        }
        let acp_session_id = connection.session_id().to_string();
        let options = connection.config_options().to_vec();
        let commands = connection
            .commands()
            .iter()
            .map(|command| serde_json::to_value(command).unwrap_or_default())
            .collect::<Vec<_>>();
        if self.flavor == NativeChatFlavor::DeepSeekHarness {
            live.view.lock().map_err(|_| anyhow!("{} 视图锁已损坏", self.flavor.label()))?.control_state = connection.native_control("state", "")?;
            *live.control_handle.lock().map_err(|_| anyhow!("{} 控制锁已损坏", self.flavor.label()))? = connection.native_control_handle().map(|control| (control, acp_session_id.clone()));
        } else {
            live.replaying.store(resumed_expected, Ordering::Release);
            *live.cancel_handle.lock().map_err(|_| anyhow!("ACP 取消句柄锁已损坏"))? = Some(connection.cancel_handle());
        }
        self.mutate_store(|store| {
            let entry = store
                .sessions
                .get_mut(&record.id)
                .ok_or_else(|| anyhow!("会话定位已删除"))?;
            if entry.cwd != locator.cwd || entry.permission_mode != locator.permission_mode {
                return Err(anyhow!("会话定位在启动期间发生变化"));
            }
            if let Some(expected) = &locator.acp_session_id {
                // Claude Code 每次 resume 都会产生新的上游会话文件，允许替换链接；
                // dsh 的 session/resume 语义不同，保持拒绝替换。
                if expected != &acp_session_id && self.flavor == NativeChatFlavor::DeepSeekHarness {
                    return Err(anyhow!("ACP 恢复返回了不同的会话 ID，拒绝替换原会话"));
                }
            }
            entry.cwd = cwd.clone();
            if self.flavor == NativeChatFlavor::CodexChat {
                // Keep saved usage only for the same upstream thread. New/fallback
                // sessions must never inherit occupancy from their predecessor.
                if entry.acp_session_id.as_deref() != Some(acp_session_id.as_str()) {
                    entry.context_usage = None;
                    entry.codex_goal = None;
                }
                if crate::acp::context_model(&entry.config_options) != crate::acp::context_model(&options) {
                    entry.context_usage = None;
                }
                if let Some(usage) = connection.context_usage() {
                    entry.context_usage = usage;
                }
                if let Some(goal) = connection.goal() { entry.codex_goal = goal; }
            }
            entry.acp_session_id = Some(acp_session_id.clone());
            entry.config_options = options.clone();
            entry.commands = commands.clone();
            Ok(())
        })?;
        if self.flavor == NativeChatFlavor::CodexChat {
            let handle = connection.goal_handle();
            live.view.lock().map_err(|_| anyhow!("Codex 视图锁已损坏"))?.goal_supported = handle.is_some();
            *live.goal_handle.lock().map_err(|_| anyhow!("Codex 目标句柄锁已损坏"))? = handle;
        }
        let manager = self.clone();
        let event_app = app.clone();
        let session_id = record.id.clone();
        let title_sessions = sessions.clone();
        let event_live = Arc::clone(&live);
        let stream = if self.flavor == NativeChatFlavor::DeepSeekHarness {
            let stream = connection.native_control_handle().ok_or_else(|| anyhow!("{} 实时控制句柄缺失", self.flavor.label()))?.subscribe(&acp_session_id)?;
            live.stream_authoritative.store(true, Ordering::Release);
            live.stream_revision.store(0, Ordering::Release);
            *live.stream_failure.lock().map_err(|_| anyhow!("{} 实时流错误锁损坏", self.flavor.label()))? = None;
            Some(stream)
        } else {
            None
        };
        connection.set_update_handler(Box::new(move |update| {
            if event_live.generation.load(Ordering::Acquire) != generation {
                return;
            }
            // Kimi 此通知还包含首条输入形成的 replaceable 名称，不能标记为摘要成功，
            // 否则首轮结束后的原生 chat_title 任务会被去重规则跳过。
            if update["sessionUpdate"] == "session_info_update" && manager.flavor != NativeChatFlavor::KimiChat {
                if let Some(title) = update["title"].as_str().filter(|title| !title.trim().is_empty()) {
                    match title_sessions.apply_summary_title(&session_id, title) {
                        Ok(Some(record)) => manager.emit(&event_app, &event_live, &session_id, "state", json!({"title": title}), Some(record)),
                        Err(error) => eprintln!("[crewvela] 保存原生会话标题失败 session={session_id}: {error:#}"),
                        _ => {}
                    }
                }
            }
            match update.get("sessionUpdate").and_then(Value::as_str) {
                Some("session_info_update") if manager.flavor == NativeChatFlavor::CodexChat => {
                    if let Some(status) = update.pointer("/_meta/codex/threadStatus/type").and_then(Value::as_str).filter(|value| matches!(*value, "active" | "idle" | "notLoaded" | "systemError")) {
                        let active = status == "active";
                        event_live.codex_thread_reported.store(true, Ordering::Release);
                        event_live.codex_thread_active.store(active, Ordering::Release);
                        if !event_live.prompt_inflight.load(Ordering::Acquire) {
                            if active {
                                event_live.busy.store(true, Ordering::Release);
                                event_live.goal_executing.store(true, Ordering::Release);
                                event_live.ensure_codex_goal_turn();
                            } else {
                                let persistence_failed = manager.flush_active_delta(&event_app, &event_live, &session_id).is_err()
                                    || event_live.stream_failure.lock().unwrap().is_some();
                                // Cancellation belongs to the completed turn, not the
                                // live session. The generation guard above excludes stop.
                                let cancelled = event_live.cancelled.load(Ordering::Acquire);
                                let turn = event_live.active_turn_id.lock().ok().and_then(|mut turn| turn.take());
                                if let Some(turn) = turn {
                                    let outcome = if persistence_failed || status == "systemError" { SessionStatus::Error }
                                        else if cancelled || status == "notLoaded" { SessionStatus::Stopped }
                                        else { SessionStatus::Completed };
                                    if let Err(error) = title_sessions.record_native_turn_outcome(&session_id, &turn, outcome) {
                                        manager.emit(&event_app, &event_live, &session_id, "error", json!({"message":format!("记录目标轮次事件失败：{error:#}")}), None);
                                    }
                                }
                                event_live.cancelled.store(false, Ordering::Release);
                                event_live.busy.store(false, Ordering::Release);
                                event_live.goal_executing.store(false, Ordering::Release);
                                manager.emit(&event_app, &event_live, &session_id, "turn_end", json!({"goal": true}), None);
                            }
                        }
                        if status == "systemError" { manager.emit(&event_app, &event_live, &session_id, "codex_goal_error", json!({"message":"Codex 原生线程发生错误，请暂停目标或恢复会话"}), None); }
                        manager.emit(&event_app, &event_live, &session_id, "codex_goal", update.clone(), None);
                    }
                    if let Some(value) = update.pointer("/_meta/goal") {
                        let goal = crate::acp::parse_codex_goal(value);
                        if manager.mutate_store(|store| {
                            store.sessions.get_mut(&session_id).ok_or_else(|| anyhow!("Codex 会话定位已删除"))?.codex_goal = goal;
                            Ok(())
                        }).is_err() { event_live.cancelled.store(true, Ordering::Release); return; }
                        manager.emit(&event_app, &event_live, &session_id, "codex_goal", update.clone(), None);
                    }
                }
                Some("agent_message_chunk") if !event_live.stream_authoritative.load(Ordering::Acquire) => {
                    // Claude session/load 后的整段历史重放与本地持久化重复，丢弃。
                    if event_live.replaying.load(Ordering::Acquire) {
                        return;
                    }
                    if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                        let turn_id = event_live
                            .active_turn_id
                            .lock()
                            .ok()
                            .map(|mut id| id.get_or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone());
                        if let Some(turn_id) = turn_id {
                            let Ok(mut pending) = event_live.pending_delta.lock() else { return; };
                            if pending.0.is_empty() {
                                pending.1 = std::time::Instant::now();
                            }
                            pending.0.push_str(text);
                            let should_flush = pending.0.len() >= 1024
                                || pending.1.elapsed() >= Duration::from_millis(200);
                            drop(pending);
                            if should_flush && manager.flush_assistant_delta(&event_app, &event_live, &session_id, &turn_id).is_err() {
                                event_live.cancelled.store(true, Ordering::Release);
                                return;
                            }
                        }
                    }
                }
                Some("agent_thought_chunk") if !event_live.stream_authoritative.load(Ordering::Acquire) => {
                    // codex-acp 等标准 ACP 适配器用 thought chunk 流式输出思考；
                    // 回放期间与消息 chunk 一同丢弃。
                    if event_live.replaying.load(Ordering::Acquire) {
                        return;
                    }
                    if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                        let turn_id = event_live
                            .active_turn_id
                            .lock()
                            .ok()
                            .map(|mut id| id.get_or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone());
                        if let Some(turn_id) = turn_id {
                            let should_flush = {
                                let Ok(mut reasoning) = event_live.pending_reasoning.lock() else { return; };
                                reasoning.push_str(text);
                                reasoning.len() >= 1024
                            };
                            if should_flush && manager.flush_assistant_delta(&event_app, &event_live, &session_id, &turn_id).is_err() {
                                event_live.cancelled.store(true, Ordering::Release);
                                return;
                            }
                        }
                    }
                }
                Some("tool_call" | "tool_call_update") => {
                    if event_live.replaying.load(Ordering::Acquire) {
                        return;
                    }
                    let turn_id = event_live.active_turn_id.lock().ok().and_then(|id| id.clone());
                    if let Some(turn_id) = turn_id {
                        if manager.flush_assistant_delta(&event_app, &event_live, &session_id, &turn_id).is_err() {
                            event_live.cancelled.store(true, Ordering::Release);
                            return;
                        }
                    }
                    manager.emit(&event_app, &event_live, &session_id, "tool_call", update.clone(), None);
                }
                Some("usage_update") if manager.flavor == NativeChatFlavor::CodexChat => {
                    let usage = crate::acp::parse_context_usage(update);
                    if manager.mutate_store(|store| {
                        let locator = store.sessions.get_mut(&session_id).ok_or_else(|| anyhow!("Codex 会话定位已删除"))?;
                        locator.context_usage = usage;
                        Ok(())
                    }).is_err() { event_live.cancelled.store(true, Ordering::Release); return; }
                    manager.emit(&event_app, &event_live, &session_id, "context_usage", update.clone(), None);
                }
                Some("config_option_update") => {
                    if let Some(options) = update.get("configOptions").and_then(Value::as_array) {
                        if manager.mutate_store(|store| {
                            let locator = store.sessions.get_mut(&session_id).ok_or_else(|| anyhow!("{} 会话定位已删除", manager.flavor.label()))?;
                            if manager.flavor == NativeChatFlavor::CodexChat && crate::acp::context_model(&locator.config_options) != crate::acp::context_model(options) {
                                locator.context_usage = None;
                            }
                            locator.config_options = options.clone(); Ok(())
                        }).is_err() { event_live.cancelled.store(true, Ordering::Release); return; }
                        manager.emit(&event_app, &event_live, &session_id, "config_options", update.clone(), None);
                    }
                }
                Some("available_commands_update") => {
                    let commands = crate::acp::parse_commands(update)
                        .iter()
                        .map(|command| serde_json::to_value(command).unwrap_or_default())
                        .collect::<Vec<_>>();
                    if manager
                        .mutate_store(|store| {
                            let locator = store.sessions.get_mut(&session_id).ok_or_else(|| anyhow!("会话定位已删除"))?;
                            locator.commands = commands.clone();
                            Ok(())
                        })
                        .is_err()
                    {
                        event_live.cancelled.store(true, Ordering::Release);
                        return;
                    }
                    manager.emit(&event_app, &event_live, &session_id, "commands", update.clone(), None);
                }
                _ => {}
            }
        }));
        if live.generation.load(Ordering::Acquire) != generation {
            connection.terminate();
            return Err(anyhow!("{} 会话启动已取消", self.flavor.label()));
        }
        let _publication = live.publication.lock().map_err(|_| anyhow!("{} 发布锁已损坏", self.flavor.label()))?;
        let updated_record = sessions.mark_native_runtime_status(
            &record.id,
            SessionStatus::Running,
            None,
            None,
        )?;
        if live.generation.load(Ordering::Acquire) != generation {
            connection.terminate();
            return Err(anyhow!("{} 会话启动已取消", self.flavor.label()));
        }
        *connection_slot = Some(connection);
        live.connected.store(true, Ordering::Release);
        self.emit_unlocked(
            &app,
            &live,
            &record.id,
            "state",
            json!({"status": "running", "configOptions": options}),
            Some(updated_record),
        );
        drop(connection_slot);
        drop(_publication);
        if let Some(stream) = stream {
            self.watch_stream(stream, app.clone(), sessions.clone(), record.id.clone(), Arc::clone(&live), generation);
        }
        if self.flavor == NativeChatFlavor::DeepSeekHarness {
            self.watch_connection(app.clone(), sessions.clone(), record.id.clone(), Arc::clone(&live), generation);
        } else {
            self.watch_claude_liveness(app.clone(), sessions.clone(), record.id.clone(), Arc::clone(&live), generation);
        }
        self.snapshot(&record.id)
    }

    fn watch_connection<R: tauri::Runtime>(&self, app: AppHandle<R>, sessions: SessionManager, id: String, live: Arc<LiveSession>, generation: u64) {
        let manager = self.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            if live.generation.load(Ordering::Acquire) != generation || !live.connected.load(Ordering::Acquire) { break; }
            let Ok(_lifecycle) = live.lifecycle.try_lock() else { continue; };
            if live.generation.load(Ordering::Acquire) != generation || !live.connected.load(Ordering::Acquire) { break; }
            if live.busy.load(Ordering::Acquire) {
                if manager.flush_active_delta(&app, &live, &id).is_err() {
                    live.cancelled.store(true, Ordering::Release);
                }
            }
            if live.prompt_inflight.load(Ordering::Acquire) { continue; }
            let Ok(mut slot) = live.connection.try_lock() else { continue; };
            let Some(connection) = slot.as_mut() else { break; };
            let failure = match connection.has_exited() {
                Ok(false) => {
                    match connection.native_control("state", "").and_then(|state| {
                        connection.poll_native_updates()?;
                        Ok(state)
                    }) {
                        Ok(mut state) => {
                            if state.pointer("/sessionTitle/source/kind").and_then(Value::as_str) == Some("provider") {
                                if let Some(title) = state.pointer("/sessionTitle/title").and_then(Value::as_str) {
                                    match sessions.apply_summary_title(&id, title) {
                                        Ok(Some(record)) => manager.emit(&app, &live, &id, "state", json!({"title":title}), Some(record)),
                                        Err(error) => eprintln!("[crewvela] Harness 标题保存失败 session={id}: {error:#}"),
                                        _ => {}
                                    }
                                }
                            }
                            let busy = state["agentStatus"] == "running" || state["commandBusy"] == true;
                            let changed = {
                                let Ok(mut view) = live.view.lock() else { break; };
                                if let Some(execution) = view.control_state.get("execution").filter(|value| !value.is_null()) {
                                    state["execution"] = execution.clone();
                                }
                                let changed = view.control_state != state;
                                view.control_state = state;
                                changed
                            };
                            live.busy.store(busy, Ordering::Release);
                            if !busy {
                                live.cancelled.store(false, Ordering::Release);
                                if manager.flush_active_delta(&app, &live, &id).is_err() { live.cancelled.store(true, Ordering::Release); }
                                if let Ok(mut turn) = live.active_turn_id.lock() { *turn = None; }
                            }
                            if changed { manager.emit(&app, &live, &id, "control_state", json!({}), None); }
                            continue;
                        }
                        Err(error) => format!("{} 控制连接失败：{error:#}", manager.flavor.label()),
                    }
                },
                Ok(true) => "DeepSeek Harness ACP 进程已退出，请恢复会话".to_string(),
                Err(error) => format!("检查 DeepSeek Harness ACP 进程失败：{error:#}"),
            };
            *slot = None;
            live.connected.store(false, Ordering::Release);
            live.busy.store(false, Ordering::Release);
            if let Ok(record) = sessions.mark_native_runtime_status(&id, SessionStatus::Error, None, Some(failure.clone())) {
                manager.emit(&app, &live, &id, "error", json!({"message":failure}), Some(record));
            }
            break;
        });
    }

    /// Claude 分支没有控制插件：轮询进程存活并充当空闲期的 ACP 通知泵，
    /// busy 生命周期完全由 prompt 请求驱动（请求等待期间自行泵送消息）。
    fn watch_claude_liveness<R: tauri::Runtime>(&self, app: AppHandle<R>, sessions: SessionManager, id: String, live: Arc<LiveSession>, generation: u64) {
        let manager = self.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(250));
            if live.generation.load(Ordering::Acquire) != generation || !live.connected.load(Ordering::Acquire) { break; }
            let Ok(_lifecycle) = live.lifecycle.try_lock() else { continue; };
            if live.generation.load(Ordering::Acquire) != generation || !live.connected.load(Ordering::Acquire) { break; }
            if live.busy.load(Ordering::Acquire) || manager.flavor == NativeChatFlavor::CodexChat {
                if manager.flush_active_delta(&app, &live, &id).is_err() {
                    live.cancelled.store(true, Ordering::Release);
                }
            }
            if live.prompt_inflight.load(Ordering::Acquire) { continue; }
            let Ok(mut slot) = live.connection.try_lock() else { continue; };
            let failure = {
                let Some(connection) = slot.as_mut() else { break; };
                if let Err(error) = connection.poll_native_updates() {
                    format!("{} ACP 消息泵失败：{error:#}", manager.flavor.label())
                } else {
                    match connection.has_exited() {
                        Ok(false) => continue,
                        Ok(true) => format!("{} ACP 进程已退出，请恢复会话", manager.flavor.label()),
                        Err(error) => format!("检查{} ACP 进程失败：{error}", manager.flavor.label()),
                    }
                }
            };
            *slot = None;
            live.connected.store(false, Ordering::Release);
            live.busy.store(false, Ordering::Release);
            if let Ok(record) = sessions.mark_native_runtime_status(&id, SessionStatus::Error, None, Some(failure.clone())) {
                manager.emit(&app, &live, &id, "error", json!({"message": failure}), Some(record));
            }
            break;
        });
    }

    fn watch_stream<R: tauri::Runtime>(&self, mut reader: std::io::BufReader<std::net::TcpStream>, app: AppHandle<R>, sessions: SessionManager, id: String, live: Arc<LiveSession>, generation: u64) {
        let manager = self.clone();
        let epoch = live.stream_epoch.fetch_add(1, Ordering::AcqRel) + 1;
        std::thread::spawn(move || {
            let mut line = Vec::new();
            let mut failure = loop {
                if live.generation.load(Ordering::Acquire) != generation || live.stream_epoch.load(Ordering::Acquire) != epoch || !live.connected.load(Ordering::Acquire) { return; }
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break "Harness 实时流已断开，请恢复会话".to_string(),
                    Ok(_) if line.len() > 16 * 1024 * 1024 => break "Harness 实时流消息超过限制".to_string(),
                    Ok(_) if line.ends_with(b"\n") => {
                        let result = serde_json::from_slice::<Value>(&line).map_err(anyhow::Error::from).and_then(|frame| manager.apply_stream_message(&app, &live, &id, generation, epoch, &frame));
                        line.clear();
                        if let Err(error) = result { break format!("{} 实时流处理失败：{error:#}", manager.flavor.label()); }
                    }
                    Ok(_) => {},
                    Err(error) if matches!(error.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                        if line.len() > 16 * 1024 * 1024 { break "Harness 实时流消息超过限制".to_string(); }
                    },
                    Err(error) => break format!("{} 实时流读取失败：{error}", manager.flavor.label()),
                }
            };
            let Ok(_lifecycle) = live.lifecycle.lock() else { return; };
            // Intentional prompt cancellation can close TCP before the worker publishes
            // Stopped. Do not turn that expected close into a competing Error.
            if live.generation.load(Ordering::Acquire) != generation
                || live.stream_epoch.load(Ordering::Acquire) != epoch
                || !live.connected.load(Ordering::Acquire)
                || (live.cancelled.load(Ordering::Acquire) && live.prompt_inflight.load(Ordering::Acquire)) { return; }
            *live.stream_failure.lock().unwrap_or_else(|error| error.into_inner()) = Some(failure.clone());
            live.cancelled.store(true, Ordering::Release);
            terminate_acp_process(&live.process_id);
            if let Ok(mut slot) = live.connection.lock() { *slot = None; }
            if let Ok(mut control) = live.control_handle.lock() { *control = None; }
            if let Ok(mut cancel) = live.cancel_handle.lock() { *cancel = None; }
            let Ok(_publication) = live.publication.lock() else { return; };
            if let Err(error) = manager.interrupt_stream_messages(&id) { failure.push_str(&format!("；保存部分消息失败：{error:#}")); }
            if let Ok(record) = sessions.mark_native_runtime_status(&id, SessionStatus::Error, None, Some(failure.clone())) {
                live.connected.store(false, Ordering::Release);
                live.busy.store(false, Ordering::Release);
                manager.emit_unlocked(&app, &live, &id, "error", json!({"message": failure}), Some(record));
            }
        });
    }

    fn apply_stream_message<R: tauri::Runtime>(&self, app: &AppHandle<R>, live: &LiveSession, session_id: &str, generation: u64, epoch: u64, frame: &Value) -> Result<()> {
        let message = frame.get("assistant").ok_or_else(|| anyhow!("{} 实时流缺少消息", self.flavor.label()))?;
        let revision = frame["revision"].as_u64().ok_or_else(|| anyhow!("{} 实时流版本缺失", self.flavor.label()))?;
        let upstream_id = message["id"].as_str().ok_or_else(|| anyhow!("{} 实时消息缺少标识", self.flavor.label()))?;
        let text = message["text"].as_str().ok_or_else(|| anyhow!("{} 实时消息正文无效", self.flavor.label()))?;
        let reasoning = message["reasoning"].as_str().ok_or_else(|| anyhow!("{} 实时消息思考无效", self.flavor.label()))?;
        let status = message["streamStatus"].as_str().filter(|status| matches!(*status, "streaming" | "completed" | "failed" | "interrupted" | "abandoned")).ok_or_else(|| anyhow!("{} 实时消息状态无效", self.flavor.label()))?;
        let _publication = live.publication.lock().map_err(|_| anyhow!("{} 发布锁损坏", self.flavor.label()))?;
        if live.generation.load(Ordering::Acquire) != generation || live.stream_epoch.load(Ordering::Acquire) != epoch { return Ok(()); }
        // 取消/停止拆除期间迟到的终态帧不得把 interrupted 改写回 completed。
        if live.cancelled.load(Ordering::Acquire) { return Ok(()); }
        if revision <= live.stream_revision.load(Ordering::Acquire) { return Ok(()); }
        let id = format!("stream-{generation}-{epoch}-{upstream_id}");
        self.mutate_store(|store| {
            let locator = store.sessions.get_mut(session_id).ok_or_else(|| anyhow!("{} 会话不存在", self.flavor.label()))?;
            if let Some(existing) = locator.history.iter_mut().find(|item| item.id == id) {
                existing.text = text.to_string();
                existing.reasoning = reasoning.to_string();
                existing.stream_status = status.to_string();
            } else {
                locator.history.push(HarnessChatMessage { id: id.clone(), role: "assistant".into(), text: text.into(), reasoning: reasoning.into(), stream_status: status.into(), created_at: now_millis(), attachments: Vec::new() });
            }
            Ok(())
        })?;
        self.emit_unlocked(app, live, session_id, "assistant_stream", message.clone(), None);
        live.stream_revision.store(revision, Ordering::Release);
        Ok(())
    }

    pub fn set_model(&self, session_id: &str, value: &str) -> Result<HarnessSessionSnapshot> {
        self.set_config(session_id, "model", value)
    }

    pub fn set_config(&self, session_id: &str, id: &str, value: &str) -> Result<HarnessSessionSnapshot> {
        let live = self.live_session(session_id)?;
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("{} 生命周期锁已损坏", self.flavor.label()))?;
        if live.busy.load(Ordering::Acquire) {
            return Err(anyhow!("{} 正在处理消息，暂不能切换模型", self.flavor.label()));
        }
        let mut slot = live
            .connection
            .lock()
            .map_err(|_| anyhow!("{} 连接锁已损坏", self.flavor.label()))?;
        let connection = slot
            .as_mut()
            .ok_or_else(|| anyhow!("{} 会话尚未连接", self.flavor.label()))?;
        let options = if self.flavor == NativeChatFlavor::DeepSeekHarness {
            let state = connection.native_control("state", "")?;
            if state["agentStatus"] == "running" || state["commandBusy"] == true { return Err(anyhow!("{} 正在运行，暂不能切换配置", self.flavor.label())); }
            if id == "model" { connection.set_deepseek_model(value)? } else { connection.set_config_option(id, value)? }
        } else {
            connection.set_config_option(id, value)?
        };
        self.mutate_store(|store| {
            let locator = store
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| anyhow!("{} 会话定位已删除", self.flavor.label()))?;
            locator.config_options = options.clone();
            if self.flavor == NativeChatFlavor::CodexChat && id == "model" {
                locator.context_usage = None;
            }
            Ok(())
        })?;
        drop(slot);
        self.snapshot(session_id)
    }

    pub fn send_message<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        sessions: &SessionManager,
        session_id: &str,
        message: &str,
    ) -> Result<()> {
        self.send_message_with_attachments(app, sessions, session_id, message, &[])
    }

    pub fn control<R: tauri::Runtime>(&self, app: &AppHandle<R>, session_id: &str, action: &str, value: &str) -> Result<HarnessSessionSnapshot> {
        if !matches!(action, "command" | "permission" | "state") || value.len() > 32_000 {
            return Err(anyhow!("无效的 Harness 控制请求"));
        }
        if self.flavor != NativeChatFlavor::DeepSeekHarness {
            return Err(anyhow!("{}暂不支持控制命令", self.flavor.label()));
        }
        if action == "command" && !value.starts_with("/goal") && value != "/compact" {
            return Err(anyhow!("暂不支持该 Harness 斜杠命令"));
        }
        let live = self.live_session(session_id)?;
        let generation = live.generation.load(Ordering::Acquire);
        let (control, id) = live.control_handle.lock().map_err(|_| anyhow!("{} 控制锁已损坏", self.flavor.label()))?.clone().ok_or_else(|| anyhow!("{} 会话未连接", self.flavor.label()))?;
        if matches!(value, "/goal pause" | "/goal clear") { self.reject_pending_permissions(&live); }
        if action == "command" { live.busy.store(true, Ordering::Release); }
        self.emit(app, &live, session_id, "control_state", json!({}), None);
        let result = control.request(&id, action, value);
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("{} 生命周期锁已损坏", self.flavor.label()))?;
        if generation != live.generation.load(Ordering::Acquire) || !live.connected.load(Ordering::Acquire) {
            return Err(anyhow!("{} 控制请求期间会话已停止，结果不会覆盖新连接", self.flavor.label()));
        }
        let state = match result {
            Ok(state) => state,
            Err(error) => {
                if let Ok(state) = control.request(&id, "state", "") {
                    live.busy.store(state["agentStatus"] == "running" || state["commandBusy"] == true, Ordering::Release);
                    live.view.lock().map_err(|_| anyhow!("{} 视图锁已损坏", self.flavor.label()))?.control_state = state;
                }
                self.emit(app, &live, session_id, "control_state", json!({}), None);
                return Err(error);
            }
        };
        live.busy.store(state["agentStatus"] == "running" || state["commandBusy"] == true, Ordering::Release);
        live.view.lock().map_err(|_| anyhow!("{} 视图锁已损坏", self.flavor.label()))?.control_state = state;
        self.emit(app, &live, session_id, "control_state", json!({}), None);
        self.snapshot(session_id)
    }

    pub fn send_message_with_attachments<R: tauri::Runtime>(
        &self, app: AppHandle<R>, sessions: &SessionManager,
        session_id: &str, message: &str, paths: &[String],
    ) -> Result<()> {
        self.send_codex_message_with_plugins(app, sessions, session_id, message, paths, &[])
    }

    pub fn codex_plugins(&self, session_id: &str) -> Result<Value> {
        if self.flavor != NativeChatFlavor::CodexChat { return Err(anyhow!("仅 Codex Chat 支持原生插件")); }
        let live = self.live_session(session_id)?;
        if !live.connected.load(Ordering::Acquire) { return Err(anyhow!("请先恢复 Codex 会话")); }
        let mut slot = live.connection.try_lock().map_err(|_| anyhow!("Codex 正在执行，请结束或暂停后刷新插件"))?;
        slot.as_mut().context("Codex 会话未连接")?.codex_plugins()
    }

    pub fn send_codex_message_with_plugins<R: tauri::Runtime>(
        &self, app: AppHandle<R>, sessions: &SessionManager,
        session_id: &str, message: &str, paths: &[String], plugins: &[String],
    ) -> Result<()> {
        validate_cc_session_id(session_id)?;
        let message = message.trim().to_string();
        // The public slash command and composer menu share the same goal
        // lifecycle. A goal must never become an ordinary manual-prompt worker.
        if self.flavor == NativeChatFlavor::CodexChat && message.strip_prefix("/goal").is_some_and(|rest| rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace)) {
            if !paths.is_empty() || !plugins.is_empty() { return Err(anyhow!("目标命令不能同时发送附件或插件，请使用普通消息")); }
            let argument = message.strip_prefix("/goal").unwrap_or_default().trim();
            if argument.is_empty() { return Err(anyhow!("请输入 /goal <目标内容> 或使用目标菜单")); }
            return match argument.to_ascii_lowercase().as_str() {
                "pause" => self.control_codex_goal(app, session_id, "pause", None),
                "resume" => self.control_codex_goal(app, session_id, "resume", None),
                "clear" => self.control_codex_goal(app, session_id, "clear", None),
                _ => self.control_codex_goal(app, session_id, "set", Some(argument)),
            };
        }
        let attachments = normalize_attachments(paths)?;
        if plugins.len() > 20 || plugins.iter().any(|id| !valid_codex_plugin_id(id)) { return Err(anyhow!("无效的 Codex 插件引用")); }
        if !plugins.is_empty() && self.flavor != NativeChatFlavor::CodexChat { return Err(anyhow!("仅 Codex Chat 支持原生插件引用")); }
        if (message.is_empty() && attachments.is_empty() && plugins.is_empty()) || message.chars().count() > PROMPT_MAX_CHARS {
            return Err(anyhow!("消息不能为空且不能超过 {PROMPT_MAX_CHARS} 个字符"));
        }
        let live = self.live_session(session_id)?;
        if self.flavor == NativeChatFlavor::CodexChat && (live.busy.load(Ordering::Acquire) || self.locator(session_id)?.codex_goal.is_some_and(|goal| goal.status == "active")) {
            return Err(anyhow!("Codex 正在执行，请先暂停或清除目标后发送新任务"));
        }
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("{} 生命周期锁已损坏", self.flavor.label()))?;
        // Catalog RPC pumps native session notifications. Do not hold the
        // publication lock while those callbacks may emit another snapshot.
        if !plugins.is_empty() {
            let mut slot = live.connection.lock().map_err(|_| anyhow!("Codex 连接锁已损坏"))?;
            let catalog = slot.as_mut().context("Codex 会话未连接")?.codex_plugins()?;
            let allowed = available_codex_plugin_ids(&catalog);
            if !plugins.iter().all(|id| allowed.contains(id)) { return Err(anyhow!("所选插件已停用或不可用，请刷新插件列表")); }
        }
        let _publication = live.publication.lock().map_err(|_| anyhow!("{} 发布锁已损坏", self.flavor.label()))?;
        if self.flavor == NativeChatFlavor::CodexChat && self.locator(session_id)?.codex_goal.is_some_and(|goal| goal.status == "active") {
            return Err(anyhow!("Codex 正在追求目标，请先暂停或清除目标后发送新任务"));
        }
        if live
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(anyhow!("{} 正在处理上一条消息", self.flavor.label()));
        }
        let mut connection_slot = live
            .connection
            .lock()
            .map_err(|_| anyhow!("{} 连接锁已损坏", self.flavor.label()))?;
        if let Some(connection) = connection_slot.as_mut() {
            if self.flavor == NativeChatFlavor::DeepSeekHarness {
                let action = if message.starts_with('/') { "skill" } else { "state" };
                match connection.native_control(action, if action == "skill" { &message } else { "" }) {
                    Ok(state) if state["agentStatus"] == "running" || state["commandBusy"] == true => {
                        return Err(anyhow!("{} 目标或会话正在运行，请先暂停目标或取消本轮", self.flavor.label()));
                    }
                    Err(error) => { live.busy.store(false, Ordering::Release); return Err(error); }
                    _ => {}
                }
            }
        } else {
            live.busy.store(false, Ordering::Release);
            return Err(anyhow!("{} 会话尚未连接，请先恢复会话", self.flavor.label()));
        }
        drop(connection_slot);
        let turn_id = uuid::Uuid::new_v4().to_string();
        if let Err(error) = self.append_user_message(session_id, &turn_id, &message, &attachments) {
            live.busy.store(false, Ordering::Release);
            return Err(error);
        }
        if self.flavor == NativeChatFlavor::CodexChat && message == "/clear" {
            if let Err(error) = self.mutate_store(|store| {
                store.sessions.get_mut(session_id).ok_or_else(|| anyhow!("Codex 会话定位已删除"))?.context_usage = None;
                Ok(())
            }) {
                live.busy.store(false, Ordering::Release);
                return Err(error);
            }
        }
        *live
            .active_turn_id
            .lock()
            .map_err(|_| anyhow!("{} 消息状态锁已损坏", self.flavor.label()))? = Some(turn_id.clone());
        live.cancelled.store(false, Ordering::Release);
        live.cancel_requested.store(false, Ordering::Release);
        *live.stream_failure.lock().unwrap() = None;
        // 首次真实发送后解除 load 回放护栏。
        live.replaying.store(false, Ordering::Release);
        let generation = live.generation.load(Ordering::Acquire);
        let app_for_worker = app.clone();
        let sessions_for_worker = sessions.clone();
        let manager = self.clone();
        let session_id = session_id.to_string();
        let live_for_worker = Arc::clone(&live);
        let plugins = plugins.to_vec();
        self.emit_unlocked(&app, &live, &session_id, "user_message", json!({"id": turn_id, "text": message}), None);
        live.prompt_inflight.store(true, Ordering::Release);
        std::thread::spawn(move || {
            let mut result = {
                if live_for_worker.generation.load(Ordering::Acquire) != generation { return; }
                let mut slot = match live_for_worker.connection.lock() {
                    Ok(slot) => slot,
                    Err(_) => return,
                };
                if live_for_worker.generation.load(Ordering::Acquire) != generation { return; }
                let Some(connection) = slot.as_mut() else {
                    return;
                };
                let mut content = Vec::new();
                if !message.is_empty() { content.push(json!({"type":"text","text":message})); }
                content.extend(attachments.iter().map(|file| json!({"type":"resource_link","name":file.name,"uri":file.uri})));
                content.extend(plugins.iter().map(|id| json!({"type":"resource_link","name":id.split('@').next().unwrap_or(id),"uri":format!("plugin://{id}")})));
                let mut result = connection.prompt_content_interactive(content, &live_for_worker.cancelled);
                if manager.flavor == NativeChatFlavor::DeepSeekHarness && result.is_ok() && !live_for_worker.cancelled.load(Ordering::Acquire) {
                    match connection.native_control("state", "") {
                    Ok(state) => {
                        let target = state["streamRevision"].as_u64().unwrap_or(0);
                        let deadline = std::time::Instant::now() + Duration::from_secs(5);
                        while live_for_worker.stream_revision.load(Ordering::Acquire) < target && live_for_worker.connected.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
                            std::thread::sleep(Duration::from_millis(10));
                        }
                        if live_for_worker.stream_revision.load(Ordering::Acquire) < target { result = Err(anyhow!("{} 实时流最终结果未同步，请恢复会话", manager.flavor.label())); }
                    },
                    Err(error) => result = Err(error.context("Harness 实时流最终同步失败")),
                    }
                }
                result
            };
            let Ok(_lifecycle) = live_for_worker.lifecycle.lock() else { return; };
            if let Some(failure) = live_for_worker.stream_failure.lock().ok().and_then(|failure| failure.clone()) { result = Err(anyhow!(failure)); }
            let current = live_for_worker.generation.load(Ordering::Acquire) == generation;
            if current {
                live_for_worker.prompt_inflight.store(false, Ordering::Release);
                if live_for_worker.cancel_requested.load(Ordering::Acquire) { live_for_worker.cancelled.store(true, Ordering::Release); }
                if let Err(error) = manager.flush_active_delta(&app_for_worker, &live_for_worker, &session_id) {
                    *live_for_worker.stream_failure.lock().unwrap() = Some(format!("保存回复失败：{error:#}"));
                    result = Err(error.context("保存流式回复失败"));
                }
                match result {
                    Ok(AcpPromptResult { text, cancelled, .. }) => {
                        if !cancelled && !live_for_worker.cancelled.load(Ordering::Acquire) && manager.flavor != NativeChatFlavor::DeepSeekHarness {
                            let upstream = manager.locator(&session_id).ok().and_then(|locator| locator.acp_session_id);
                            let model = manager.locator(&session_id).ok().and_then(|locator| locator.config_options.iter().find(|option| option["category"] == "model" || option["id"] == "model").and_then(|option| option["currentValue"].as_str()).map(str::to_string));
                            sessions_for_worker.queue_title_summary_for_model(&session_id, &message, upstream, model);
                        }
                        if cancelled || live_for_worker.cancelled.load(Ordering::Acquire) {
                            live_for_worker.stream_epoch.fetch_add(1, Ordering::AcqRel);
                            if let Err(error) = crate::acp::terminate_acp_process_checked(&live_for_worker.process_id) {
                                live_for_worker.generation.fetch_add(1, Ordering::AcqRel);
                                live_for_worker.busy.store(true, Ordering::Release);
                                let message = format!("取消后停止未获确认，保留连接并禁止新消息：{error:#}");
                                if let Ok(record) = sessions_for_worker.mark_native_runtime_status(&session_id, SessionStatus::Error, None, Some(message.clone())) {
                                    manager.emit(&app_for_worker, &live_for_worker, &session_id, "error", json!({"message":message}), Some(record));
                                }
                                return;
                            }
                            if let Err(error) = manager.interrupt_stream_messages(&session_id) { manager.emit(&app_for_worker, &live_for_worker, &session_id, "error", json!({"message":format!("保存取消状态失败：{error:#}")}), None); }
                            if let Ok(mut slot) = live_for_worker.connection.lock() {
                                *slot = None;
                            }
                            live_for_worker.connected.store(false, Ordering::Release);
                            if let Ok(record) = sessions_for_worker.mark_native_runtime_status(
                                &session_id,
                                SessionStatus::Stopped,
                                None,
                                None,
                            ) {
                                manager.emit(&app_for_worker, &live_for_worker, &session_id, "state", json!({"status": "stopped"}), Some(record));
                            }
                        }
                        let persistence_error = if !text.is_empty() && !live_for_worker.stream_authoritative.load(Ordering::Acquire) {
                            manager.finish_assistant_message(&session_id, &turn_id, &text).err()
                        } else { None };
                        if let Some(error) = &persistence_error {
                            manager.emit(&app_for_worker, &live_for_worker, &session_id, "error", json!({
                                "message": format!("保存回复失败：{error:#}")
                            }), None);
                        }
                        let outcome = if cancelled || live_for_worker.cancelled.load(Ordering::Acquire) {
                            SessionStatus::Stopped
                        } else if persistence_error.is_some() { SessionStatus::Error }
                        else { SessionStatus::Completed };
                        if let Err(error) = sessions_for_worker.record_native_turn_outcome(&session_id, &turn_id, outcome) {
                            manager.emit(&app_for_worker, &live_for_worker, &session_id, "error", json!({"message": format!("记录轮次事件失败：{error:#}")}), None);
                        }
                        live_for_worker.busy.store(false, Ordering::Release);
                        manager.emit(&app_for_worker, &live_for_worker, &session_id, "turn_end", json!({
                            "cancelled": cancelled || live_for_worker.cancelled.load(Ordering::Acquire)
                        }), None);
                    }
                    Err(error) if live_for_worker.cancelled.load(Ordering::Acquire) && live_for_worker.stream_failure.lock().ok().is_some_and(|failure| failure.is_none()) => {
                        live_for_worker.stream_epoch.fetch_add(1, Ordering::AcqRel);
                        if let Err(error) = crate::acp::terminate_acp_process_checked(&live_for_worker.process_id) {
                            // Keep ownership and block new prompts until an explicit stop succeeds.
                            live_for_worker.generation.fetch_add(1, Ordering::AcqRel);
                            live_for_worker.busy.store(true, Ordering::Release);
                            let message = format!("取消后停止未获确认，保留连接并禁止新消息：{error:#}");
                            if let Ok(record) = sessions_for_worker.mark_native_runtime_status(&session_id, SessionStatus::Error, None, Some(message.clone())) {
                                manager.emit(&app_for_worker, &live_for_worker, &session_id, "error", json!({"message":message}), Some(record));
                            }
                            return;
                        }
                        if let Err(error) = manager.interrupt_stream_messages(&session_id) { manager.emit(&app_for_worker, &live_for_worker, &session_id, "error", json!({"message":format!("保存取消状态失败：{error:#}")}), None); }
                        if let Ok(mut slot) = live_for_worker.connection.lock() {
                            *slot = None;
                        }
                        live_for_worker.connected.store(false, Ordering::Release);
                        live_for_worker.busy.store(false, Ordering::Release);
                        if let Ok(record) = sessions_for_worker.mark_native_runtime_status(
                            &session_id,
                            SessionStatus::Stopped,
                            None,
                            None,
                        ) {
                            manager.emit(&app_for_worker, &live_for_worker, &session_id, "state", json!({"status": "stopped"}), Some(record));
                        }
                        manager.emit(&app_for_worker, &live_for_worker, &session_id, "turn_end", json!({
                            "cancelled": true,
                            "message": error.to_string()
                        }), None);
                    }
                    Err(error) => {
                        if let Ok(mut slot) = live_for_worker.connection.lock() {
                            *slot = None;
                        }
                        live_for_worker.connected.store(false, Ordering::Release);
                        live_for_worker.busy.store(false, Ordering::Release);
                        if let Ok(record) = sessions_for_worker.mark_native_runtime_status(
                            &session_id,
                            SessionStatus::Error,
                            None,
                            Some(error.to_string()),
                        ) {
                            manager.emit(&app_for_worker, &live_for_worker, &session_id, "error", json!({"message": error.to_string()}), Some(record));
                        }
                    }
                }
            }
            if current {
                if let Ok(mut active_turn_id) = live_for_worker.active_turn_id.lock() {
                    *active_turn_id = None;
                }
            }
        });
        Ok(())
    }

    pub fn control_codex_goal<R: tauri::Runtime>(&self, app: AppHandle<R>, session_id: &str, action: &str, objective: Option<&str>) -> Result<()> {
        if self.flavor != NativeChatFlavor::CodexChat { return Err(anyhow!("仅 Codex Chat 支持此目标控制")); }
        if !matches!(action, "set" | "pause" | "resume" | "clear") { return Err(anyhow!("不支持的目标操作")); }
        let objective = objective.map(str::trim);
        if action == "set" && !objective.is_some_and(|value| !value.is_empty() && value.chars().count() <= 4000) {
            return Err(anyhow!("目标不能为空且不能超过 4000 个字符"));
        }
        let live = self.live_session(session_id)?;
        let _goal_submission = live.goal_submission.lock().map_err(|_| anyhow!("Codex 目标提交锁已损坏"))?;
        let generation = live.generation.load(Ordering::Acquire);
        if !live.connected.load(Ordering::Acquire) { return Err(anyhow!("请先恢复 Codex 会话")); }
        let handle = live.goal_handle.lock().map_err(|_| anyhow!("Codex 目标句柄锁已损坏"))?.clone().ok_or_else(|| anyhow!("当前 Codex ACP 适配器未公开目标控制能力，请升级适配器"))?;
        let starts_turn = matches!(action, "set" | "resume");
        if starts_turn && live.busy.load(Ordering::Acquire) { return Err(anyhow!("Codex 正在执行，请先暂停或取消当前任务")); }
        let _goal_lifecycle = if starts_turn { Some(live.lifecycle.lock().map_err(|_| anyhow!("Codex 生命周期锁已损坏"))?) } else { None };
        // Never wait behind the message pump's approval or a model turn. Pause
        // and clear can write independently through the authenticated pipe.
        if starts_turn {
            if live.generation.load(Ordering::Acquire) != generation || !live.connected.load(Ordering::Acquire) { return Err(anyhow!("Codex 会话已停止")); }
            if self.locator(session_id)?.codex_goal.is_some_and(|goal| goal.status == "active") {
                return Err(anyhow!("请先暂停现有目标再设置或恢复目标"));
            }
            if live.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
                return Err(anyhow!("Codex 正在执行，请先暂停或取消当前任务"));
            }
            live.cancelled.store(false, Ordering::Release);
            live.replaying.store(false, Ordering::Release);
            live.goal_executing.store(true, Ordering::Release);
            *live.stream_failure.lock().unwrap() = None;
            *live.active_turn_id.lock().map_err(|_| anyhow!("Codex 消息状态锁已损坏"))? = Some(uuid::Uuid::new_v4().to_string());
        } else if live.goal_executing.load(Ordering::Acquire) && !live.prompt_inflight.load(Ordering::Acquire) {
            self.reject_pending_permissions(&live);
        }
        let owns_goal_turn = live.goal_executing.load(Ordering::Acquire) && !live.prompt_inflight.load(Ordering::Acquire);
        let revision = live.goal_revision.fetch_add(1, Ordering::AcqRel) + 1;
        live.view.lock().map_err(|_| anyhow!("Codex 视图锁已损坏"))?.goal_action = Some(action.to_string());
        if live.generation.load(Ordering::Acquire) != generation || !live.connected.load(Ordering::Acquire) { return Err(anyhow!("Codex 会话已停止")); }
        // Register and WRITE under the submission lock. Waiting for the reply
        // happens separately, so a later pause can never overtake an older set.
        let reply = match handle.submit(action, objective) {
            Ok(reply) => reply,
            Err(error) => {
                live.view.lock().map_err(|_| anyhow!("Codex 视图锁已损坏"))?.goal_action = None;
                if starts_turn { live.goal_executing.store(false, Ordering::Release); live.busy.store(false, Ordering::Release); }
                return Err(error);
            }
        };
        self.emit(&app, &live, session_id, "codex_goal", json!({}), None);
        let manager = self.clone();
        let session_id = session_id.to_string();
        drop(_goal_lifecycle);
        drop(_goal_submission);
        std::thread::spawn(move || {
            if live.generation.load(Ordering::Acquire) != generation { return; }
            let timeout = if starts_turn { Duration::from_secs(30 * 60) } else { Duration::from_secs(45) };
            let result = reply.wait(timeout);
            if live.generation.load(Ordering::Acquire) != generation || live.goal_revision.load(Ordering::Acquire) != revision { return; }
            if let Ok(mut view) = live.view.lock() { view.goal_action = None; }
            match result {
                Ok(()) => {
                    if !starts_turn && owns_goal_turn && live.codex_thread_active.load(Ordering::Acquire) {
                        if let Some(cancel) = live.cancel_handle.lock().ok().and_then(|handle| handle.clone()) {
                            if let Err(error) = cancel.cancel() { manager.emit(&app, &live, &session_id, "codex_goal_error", json!({"message": format!("目标已更新，但中断当前回合失败：{error:#}")}), None); }
                        }
                    }
                    // An ACP goal response may acknowledge only turn START.
                    // Native threadStatus idle is the only completion signal.
                    manager.emit(&app, &live, &session_id, "codex_goal", json!({}), None);
                }
                Err(error) => {
                    // The server may already have committed the goal before a
                    // transport timeout. Keep controls available; never invent
                    // a paused or completed state on an unconfirmed failure.
                    if (starts_turn || owns_goal_turn) && !live.codex_thread_active.load(Ordering::Acquire) {
                        live.goal_executing.store(false, Ordering::Release);
                        live.busy.store(false, Ordering::Release);
                    }
                    manager.emit(&app, &live, &session_id, "codex_goal_error", json!({"message": format!("{error:#}")}), None);
                }
            }
        });
        Ok(())
    }

    pub fn answer_permission(&self, session_id: &str, request_id: &str, allow: bool) -> Result<()> {
        let live = self.live_session(session_id)?;
        let sender = live
            .pending_permissions
            .lock()
            .map_err(|_| anyhow!("{} 审批状态锁已损坏", self.flavor.label()))?
            .remove(request_id)
            .ok_or_else(|| anyhow!("审批请求已结束或不存在"))?;
        sender
            .send(allow)
            .map_err(|_| anyhow!("审批请求已取消"))
    }

    pub fn cancel_turn(&self, session_id: &str) -> Result<bool> {
        let live = self.live_session(session_id)?;
        if !live.busy.load(Ordering::Acquire) {
            return Ok(false);
        }
        live.cancel_requested.store(true, Ordering::Release);
        live.cancelled.store(true, Ordering::Release);
        self.reject_pending_permissions(&live);
        if let Some((control, id)) = live.control_handle.lock().map_err(|_| anyhow!("{} 控制锁已损坏", self.flavor.label()))?.clone() {
            control.request(&id, "cancel", "")?;
        } else if let Some(handle) = live.cancel_handle.lock().map_err(|_| anyhow!("ACP 取消句柄锁已损坏"))?.clone() {
            handle.cancel()?;
        }
        Ok(true)
    }

    pub(crate) fn stop_for_handoff<R: tauri::Runtime>(&self, session_id: &str, sessions: &SessionManager, app: &AppHandle<R>) -> Result<SessionRecord> { self.stop_or_archive(session_id, sessions, app, false, true) }
    pub(crate) fn handoff_permission_mode(&self, id: &str) -> Result<String> {
        let snapshot = self.snapshot(id)?;
        if let Some(mode) = snapshot.control_state.pointer("/permission/currentValue").and_then(Value::as_str) { return Ok(mode.into()); }
        for option in &snapshot.config_options {
            if matches!(option["id"].as_str(), Some("mode" | "permission-mode" | "permissionMode")) {
                if let Some(mode) = option["currentValue"].as_str() { return Ok(mode.into()); }
            }
        }
        Ok(self.locator(id)?.permission_mode)
    }
    pub fn stop<R: tauri::Runtime>(&self, session_id: &str, sessions: &SessionManager, app: &AppHandle<R>) -> Result<SessionRecord> {
        self.stop_or_archive(session_id, sessions, app, false, false)
    }

    pub fn archive<R: tauri::Runtime>(&self, session_id: &str, sessions: &SessionManager, app: &AppHandle<R>) -> Result<SessionRecord> {
        self.stop_or_archive(session_id, sessions, app, true, false)
    }

    fn stop_or_archive<R: tauri::Runtime>(&self, session_id: &str, sessions: &SessionManager, app: &AppHandle<R>, archive: bool, _verified: bool) -> Result<SessionRecord> {
        self.stop_or_archive_with(session_id, sessions, app, archive, crate::acp::terminate_acp_process_checked)
    }

    fn stop_or_archive_with<R: tauri::Runtime>(&self, session_id: &str, sessions: &SessionManager, app: &AppHandle<R>, archive: bool, terminate: impl Fn(&AtomicU32) -> Result<()>) -> Result<SessionRecord> {
        let record = sessions.get(session_id).ok_or_else(|| anyhow!("未找到会话: {session_id}"))?;
        self.ensure_record(&record)?;
        let live = self.live_session(session_id)?;
        live.generation.fetch_add(1, Ordering::AcqRel);
        live.cancelled.store(true, Ordering::Release);
        self.reject_pending_permissions(&live);
        terminate(&live.process_id)?;
        let _goal_submission = live.goal_submission.lock().map_err(|_| anyhow!("Codex 目标提交锁已损坏"))?;
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("{} 生命周期锁已损坏", self.flavor.label()))?;
        terminate(&live.process_id)?;
        let mut slot = live.connection.lock().map_err(|_| anyhow!("{} 连接锁已损坏", self.flavor.label()))?;
        *slot = None;
        drop(slot);
        *live.control_handle.lock().map_err(|_| anyhow!("{} 控制锁已损坏", self.flavor.label()))? = None;
        *live.cancel_handle.lock().map_err(|_| anyhow!("ACP 取消句柄锁已损坏"))? = None;
        *live.goal_handle.lock().map_err(|_| anyhow!("Codex 目标句柄锁已损坏"))? = None;
        live.goal_executing.store(false, Ordering::Release);
        live.codex_thread_active.store(false, Ordering::Release);
        live.codex_thread_reported.store(false, Ordering::Release);
        live.view.lock().map_err(|_| anyhow!("Codex 视图锁已损坏"))?.goal_action = None;
        let flush_result = self.flush_active_delta(app, &live, session_id).and_then(|_| self.interrupt_stream_messages(session_id));
        let _publication = live.publication.lock().map_err(|_| anyhow!("{} 发布锁已损坏", self.flavor.label()))?;
        live.busy.store(false, Ordering::Release);
        live.prompt_inflight.store(false, Ordering::Release);
        live.connected.store(false, Ordering::Release);
        if flush_result.is_ok() {
            *live.active_turn_id.lock().map_err(|_| anyhow!("{} 消息状态锁已损坏", self.flavor.label()))? = None;
        }
        let mut updated = sessions.mark_stopped(session_id)?;
        if archive && flush_result.is_ok() { updated = sessions.archive(session_id)?; }
        self.emit_unlocked(app, &live, session_id, "state", json!({"status":"stopped"}), Some(updated.clone()));
        flush_result?;
        Ok(updated)
    }

    pub fn stop_all<R: tauri::Runtime>(&self, sessions: &SessionManager, app: &AppHandle<R>) -> Result<()> {
        let ids = self
            .store
            .lock()
            .map_err(|_| anyhow!("{} 存储锁已损坏", self.flavor.label()))?
            .sessions
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for id in ids {
            if sessions.get(&id).is_some_and(|record| self.flavor.is_standalone(&record)) {
                self.stop(&id, sessions, app)?;
            }
        }
        Ok(())
    }

    pub fn delete_with(&self, session_id: &str, delete_record: impl FnOnce() -> Result<()>) -> Result<()> {
        let live = self.live_session(session_id)?;
        live.generation.fetch_add(1, Ordering::AcqRel);
        live.cancelled.store(true, Ordering::Release);
        self.reject_pending_permissions(&live);
        crate::acp::terminate_acp_process_checked(&live.process_id)?;
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("{} 生命周期锁已损坏", self.flavor.label()))?;
        crate::acp::terminate_acp_process_checked(&live.process_id)?;
        let mut slot = live.connection.lock().map_err(|_| anyhow!("{} 连接锁已损坏", self.flavor.label()))?;
        *slot = None;
        drop(slot);
        live.busy.store(false, Ordering::Release);
        live.connected.store(false, Ordering::Release);
        let mut store = self.store.lock().map_err(|_| anyhow!("{} 存储锁已损坏", self.flavor.label()))?;
        let mut next = store.clone();
        // Older failed standalone launches can have a CrewVela record without
        // an ACP locator; they must remain deletable.
        next.sessions.remove(session_id);
        save_atomic(&self.path, &next, self.flavor.label())?;
        if let Err(error) = delete_record() {
            if let Err(restore_error) = save_atomic(&self.path, &store, self.flavor.label()) {
                return Err(anyhow!("删除 CrewVela 会话失败：{error:#}；恢复 Harness 历史也失败：{restore_error:#}"));
            }
            return Err(error);
        }
        *store = next;
        drop(store);
        self.live
            .lock()
            .map_err(|_| anyhow!("{} 会话表锁已损坏", self.flavor.label()))?
            .remove(session_id);
        Ok(())
    }

    fn permission_handler<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        sessions: SessionManager,
        session_id: String,
        live: Arc<LiveSession>,
        generation: u64,
    ) -> ManagedPermissionHandler {
        let manager = self.clone();
        Box::new(move |request, cancellation| {
            if live.generation.load(Ordering::Acquire) != generation
                || cancellation.is_some_and(|flag| flag.load(Ordering::Acquire))
            {
                return Ok(false);
            }
            let request_id = uuid::Uuid::new_v4().to_string();
            let (sender, receiver) = mpsc::sync_channel(1);
            live.pending_permissions
                .lock()
                .map_err(|_| anyhow!("{} 审批状态锁已损坏", manager.flavor.label()))?
                .insert(request_id.clone(), sender);
            let reason = request
                .pointer("/params/toolCall/title")
                .and_then(Value::as_str)
                .unwrap_or("工具调用")
                .to_string();
            let record = sessions.mark_native_runtime_status(
                &session_id,
                SessionStatus::Waiting,
                Some(reason),
                None,
            )?;
            manager.emit(&app, &live, &session_id, "state", json!({"status": "waiting"}), Some(record));
            manager.emit(&app, &live, &session_id, "permission_request", json!({
                "requestId": request_id,
                "toolCall": request.pointer("/params/toolCall").cloned().unwrap_or(Value::Null),
                "options": request.pointer("/params/options").cloned().unwrap_or(Value::Array(Vec::new())),
                "default": "reject_once"
            }), None);
            let started = std::time::Instant::now();
            let decision = loop {
                if live.generation.load(Ordering::Acquire) != generation
                    || cancellation.is_some_and(|flag| flag.load(Ordering::Acquire))
                    || live.cancelled.load(Ordering::Acquire)
                {
                    break false;
                }
                let remaining = PERMISSION_WAIT.saturating_sub(started.elapsed());
                if remaining.is_zero() {
                    break false;
                }
                match receiver.recv_timeout(remaining.min(Duration::from_millis(100))) {
                    Ok(allow) => break allow,
                    Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break false,
                }
            };
            if let Ok(mut requests) = live.pending_permissions.lock() {
                requests.remove(&request_id);
            }
            if live.generation.load(Ordering::Acquire) == generation {
                if let Ok(record) = sessions.mark_native_runtime_status(
                    &session_id,
                    SessionStatus::Running,
                    None,
                    None,
                ) {
                    manager.emit(&app, &live, &session_id, "state", json!({"status": "running"}), Some(record));
                }
            }
            Ok(decision)
        })
    }

    fn append_user_message(&self, session_id: &str, id: &str, text: &str, attachments: &[HarnessAttachment]) -> Result<()> {
        self.mutate_store(|store| {
            let locator = store
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| anyhow!("{} 会话定位不存在", self.flavor.label()))?;
            locator.history.push(HarnessChatMessage {
                id: id.into(),
                role: "user".into(),
                text: text.into(),
                created_at: now_millis(),
                attachments: attachments.to_vec(), reasoning: String::new(), stream_status: String::new(),
            });
            Ok(())
        })
    }

    fn interrupt_stream_messages(&self, session_id: &str) -> Result<()> {
        self.mutate_store(|store| {
            // Failed launches may have only the outer CrewVela record. There is
            // no history to settle; active-delta persistence still fails above.
            if let Some(locator) = store.sessions.get_mut(session_id) {
                for message in &mut locator.history {
                    if message.stream_status == "streaming" { message.stream_status = "interrupted".into(); }
                }
            }
            Ok(())
        })
    }

    fn append_assistant_delta(&self, session_id: &str, turn_id: &str, text: &str, reasoning: &str) -> Result<()> {
        self.mutate_store(|store| {
            let locator = store
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| anyhow!("{} 会话定位不存在", self.flavor.label()))?;
            if let Some(message) = locator.history.iter_mut().find(|message| message.id == turn_id && message.role == "assistant") {
                message.text.push_str(text);
                message.reasoning.push_str(reasoning);
            } else {
                locator.history.push(HarnessChatMessage {
                    id: turn_id.into(),
                    role: "assistant".into(),
                    text: text.into(),
                    created_at: now_millis(),
                    attachments: Vec::new(), reasoning: reasoning.into(), stream_status: String::new(),
                });
            }
            Ok(())
        })
    }

    fn flush_active_delta<R: tauri::Runtime>(&self, app: &AppHandle<R>, live: &LiveSession, session_id: &str) -> Result<()> {
        let turn_id = live
            .active_turn_id
            .lock()
            .map_err(|_| anyhow!("{} 消息状态锁已损坏", self.flavor.label()))?
            .clone();
        if let Some(turn_id) = turn_id {
            self.flush_assistant_delta(app, live, session_id, &turn_id)?;
        }
        Ok(())
    }

    fn flush_assistant_delta<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        live: &LiveSession,
        session_id: &str,
        turn_id: &str,
    ) -> Result<()> {
        let _publication = live.publication.lock().map_err(|_| anyhow!("{} 发布锁已损坏", self.flavor.label()))?;
        let text = {
            let mut pending = live
                .pending_delta
                .lock()
                .map_err(|_| anyhow!("{} 回复缓冲锁已损坏", self.flavor.label()))?;
            let text = std::mem::take(&mut pending.0);
            pending.1 = std::time::Instant::now();
            text
        };
        let reasoning = live
            .pending_reasoning
            .lock()
            .map_err(|_| anyhow!("{} 思考缓冲锁已损坏", self.flavor.label()))
            .map(|mut buffer| std::mem::take(&mut *buffer))?;
        if text.is_empty() && reasoning.is_empty() {
            return Ok(());
        }
        if let Err(error) = self.append_assistant_delta(session_id, turn_id, &text, &reasoning) {
            *live.stream_failure.lock().unwrap() = Some(format!("保存 {} 回复失败：{error:#}", self.flavor.label()));
            if let Ok(mut pending) = live.pending_delta.lock() {
                pending.0 = format!("{text}{}", pending.0);
            }
            if let Ok(mut buffer) = live.pending_reasoning.lock() {
                *buffer = format!("{reasoning}{}", buffer);
            }
            self.emit_unlocked(app, live, session_id, "error", json!({
                "message": format!("保存 {} 回复失败：{error:#}", self.flavor.label())
            }), None);
            return Err(error);
        }
        self.emit_unlocked(app, live, session_id, "assistant_delta", json!({"id": turn_id, "text": text, "reasoning": reasoning}), None);
        Ok(())
    }

    fn finish_assistant_message(&self, session_id: &str, turn_id: &str, text: &str) -> Result<()> {
        self.mutate_store(|store| {
            let locator = store
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| anyhow!("{} 会话定位不存在", self.flavor.label()))?;
            if !locator.history.iter().any(|message| message.id == turn_id && message.role == "assistant") {
                locator.history.push(HarnessChatMessage {
                    id: turn_id.into(),
                    role: "assistant".into(),
                    text: text.into(),
                    created_at: now_millis(),
                    attachments: Vec::new(), reasoning: String::new(), stream_status: String::new(),
                });
            }
            Ok(())
        })
    }

    fn emit<R: tauri::Runtime>(
        &self, app: &AppHandle<R>, live: &LiveSession, session_id: &str,
        kind: &str, data: Value, record: Option<SessionRecord>,
    ) {
        let _publication = live.publication.lock().expect("Harness publication lock poisoned");
        self.emit_unlocked(app, live, session_id, kind, data, record);
    }

    fn emit_unlocked<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        live: &LiveSession,
        session_id: &str,
        kind: &str,
        data: Value,
        record: Option<SessionRecord>,
    ) {
        let sequence = if let Ok(mut view) = live.view.lock() {
            if kind == "user_message" {
                view.tool_calls.clear();
                view.pending_permission = None;
            } else if kind == "permission_request" {
                view.pending_permission = Some(data.clone());
            } else if kind == "state" && data.get("status").and_then(Value::as_str) == Some("running") {
                view.pending_permission = None;
            } else if kind == "turn_end" || kind == "error" || (kind == "state" && data.get("status").and_then(Value::as_str) == Some("stopped")) {
                view.pending_permission = None;
            }
            if kind == "tool_call" {
                if let Some(id) = data.get("toolCallId").and_then(Value::as_str) {
                    let cached = view.tool_calls.entry(id.to_string()).or_insert_with(|| json!({}));
                    if let (Some(cached), Some(update)) = (cached.as_object_mut(), data.as_object()) {
                        cached.extend(update.clone());
                    }
                }
            }
            view.sequence = view.sequence.saturating_add(1);
            view.sequence
        } else {
            0
        };
        if let Some(store) = app.try_state::<crate::multi_agent::MultiAgentStore>() {
            if let Err(error) = store.record_conversation_native_event(session_id, sequence, kind, &data) {
                eprintln!("[crewvela] 公共会话原生事件持久化失败：{error:#}");
            }
        }
        let _ = app.emit(self.flavor.event_name(), HarnessEvent {
            session_id: session_id.to_string(),
            sequence,
            kind: kind.to_string(),
            data,
            record,
            snapshot: self.snapshot_unlocked(session_id, live).ok(),
        });
    }

    fn reject_pending_permissions(&self, live: &LiveSession) {
        if let Ok(mut requests) = live.pending_permissions.lock() {
            for (_, sender) in requests.drain() {
                let _ = sender.send(false);
            }
        }
    }

    fn live_session(&self, session_id: &str) -> Result<Arc<LiveSession>> {
        let mut live = self
            .live
            .lock()
            .map_err(|_| anyhow!("{} 会话表锁已损坏", self.flavor.label()))?;
        Ok(Arc::clone(live.entry(session_id.to_string()).or_insert_with(|| Arc::new(LiveSession::new()))))
    }

    fn locator(&self, session_id: &str) -> Result<SessionLocator> {
        validate_cc_session_id(session_id)?;
        self.store
            .lock()
            .map_err(|_| anyhow!("{} 存储锁已损坏", self.flavor.label()))?
            .sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow!("未找到 DeepSeek Harness 会话定位"))
    }

    fn mutate_store<T>(&self, mutate: impl FnOnce(&mut PersistedSessions) -> Result<T>) -> Result<T> {
        let mut store = self
            .store
            .lock()
            .map_err(|_| anyhow!("{} 存储锁已损坏", self.flavor.label()))?;
        let mut next = store.clone();
        let result = mutate(&mut next)?;
        save_atomic(&self.path, &next, self.flavor.label())?;
        *store = next;
        Ok(result)
    }
}

fn validate_cc_session_id(session_id: &str) -> Result<()> {
    uuid::Uuid::parse_str(session_id)
        .map(|_| ())
        .map_err(|_| anyhow!("CrewVela 会话 ID 无效"))
}

fn canonical_directory(path: &str) -> Result<String> {
    crate::acp::canonical_harness_directory(path)
}

/// 为 kimi 原生聊天启动错误附加登录引导（kimi 未登录时 session/new 返回 auth_required）。
fn kimi_chat_spawn_error(error: anyhow::Error) -> anyhow::Error {
    let message = format!("{error:#}");
    if message.contains("auth_required")
        || message.contains("Authentication required")
        || message.contains("-32000")
    {
        anyhow!("Kimi Code Chat 启动失败：CLI 未登录或凭据缺失。请先在终端运行 kimi login 完成登录后重试（{message}）")
    } else {
        error
    }
}

/// 为 opencode 原生聊天启动错误附加登录引导（opencode 未认证时 providers/models 探测失败）。
fn opencode_chat_spawn_error(error: anyhow::Error) -> anyhow::Error {
    let message = format!("{error:#}");
    if message.contains("auth")
        || message.contains("Auth")
        || message.contains("login")
        || message.contains("credential")
    {
        anyhow!("OpenCode Chat 启动失败：CLI 未认证或凭据缺失。请先在终端运行 opencode auth login 完成登录后重试（{message}）")
    } else {
        error
    }
}

fn save_atomic(path: &Path, store: &PersistedSessions, label: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("创建{label}会话目录失败"))?;
    }
    let bytes = serde_json::to_vec_pretty(store).with_context(|| format!("序列化{label}会话失败"))?;
    let temp = path.with_extension(format!("native-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .with_context(|| format!("创建{label}会话临时文件失败"))?;
        file.write_all(&bytes).with_context(|| format!("写入{label}会话失败"))?;
        file.sync_all().with_context(|| format!("同步{label}会话失败"))?;
        drop(file);
        fs::rename(&temp, path).with_context(|| format!("保存{label}会话失败"))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tauri::Manager;

    #[tauri::command]
    fn native_harness_smoke_record(sessions: tauri::State<'_, SessionManager>) -> SessionRecord {
        sessions.list().into_iter().next().unwrap()
    }

    #[tauri::command]
    fn stop_session(app: AppHandle, session_id: String, sessions: tauri::State<'_, SessionManager>, manager: tauri::State<'_, NativeAcpSessions>) -> std::result::Result<SessionRecord, String> {
        manager.stop(&session_id, &sessions, &app).map_err(|e| e.to_string())
    }

    #[cfg(target_os = "windows")]
    #[test]
    #[ignore = "manual Windows WebView2 UI check; run Vite on 127.0.0.1:1432 and close the test window to finish"]
    fn native_harness_ui_without_model_calls() {
        let (root, manager, sessions, _, _) = fixture();
        let mut context = tauri::generate_context!();
        context.config_mut().build.dev_url = Some("http://127.0.0.1:1432".parse().unwrap());
        let window = &mut context.config_mut().app.windows[0];
        window.title = "Harness ACP 原生验证".into();
        window.url = tauri::WebviewUrl::App("tests/fixtures/native-harness-ui.html".into());
        window.decorations = true;
        window.maximized = false;
        let cleanup_manager = manager.clone();
        let cleanup_sessions = sessions.clone();
        tauri::Builder::default().any_thread()
            .manage(manager).manage(sessions)
            .invoke_handler(tauri::generate_handler![
                native_harness_smoke_record, stop_session,
                crate::deepseek_harness_snapshot, crate::start_deepseek_harness_session,
                crate::set_deepseek_harness_model, crate::send_deepseek_harness_message,
                crate::control_deepseek_harness_session,
                crate::answer_deepseek_harness_permission, crate::cancel_deepseek_harness_turn,
            ])
            .on_window_event(move |window, event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    if cleanup_manager.stop_all(&cleanup_sessions, window.app_handle()).is_err() { api.prevent_close(); }
                    else { window.app_handle().exit(0); }
                }
            })
            .run(context).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    fn fixture() -> (PathBuf, NativeAcpSessions, SessionManager, SessionRecord, tauri::App<tauri::test::MockRuntime>) {
        let root = std::env::temp_dir().join(format!("cc-native-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sessions = SessionManager::open_at(root.join("records.json"), root.join("events")).unwrap();
        let record = sessions.create_runtime(crate::sessions::NewRuntimeSession {
            runtime_kind: "deepseek_harness".into(), cwd: root.display().to_string(), project_id: None,
        }).unwrap();
        let mut manager = NativeAcpSessions::open_at(root.join("harness.json")).unwrap();
        manager.test_peer = Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/native-harness-acp.mjs"));
        manager.register(&record.id, &record.cwd).unwrap();
        (root, manager, sessions, record, tauri::test::mock_app())
    }

    #[test]
    fn stop_missing_history_does_not_discard_pending_reply() {
        let (root, manager, sessions, record, app) = fixture();
        manager.mutate_store(|store| { store.sessions.remove(&record.id); Ok(()) }).unwrap();
        let live = manager.live_session(&record.id).unwrap();
        *live.active_turn_id.lock().unwrap() = Some("pending".into());
        live.pending_delta.lock().unwrap().0 = "unsaved reply".into();
        assert!(manager.stop(&record.id, &sessions, app.handle()).is_err());
        assert_eq!(live.pending_delta.lock().unwrap().0, "unsaved reply");
        assert_eq!(live.active_turn_id.lock().unwrap().as_deref(), Some("pending"));
        assert!(sessions.get(&record.id).is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stop_history_settlement_preserves_messages_and_reports_write_failure() {
        let (root, manager, sessions, record, app) = fixture();
        manager.append_assistant_delta(&record.id, "turn", "saved reply", "").unwrap();
        manager.mutate_store(|store| {
            store.sessions.get_mut(&record.id).unwrap().history[0].stream_status = "streaming".into();
            Ok(())
        }).unwrap();
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let snapshot = manager.snapshot(&record.id).unwrap();
        assert_eq!(snapshot.history[0].text, "saved reply");
        assert_eq!(snapshot.history[0].stream_status, "interrupted");
        fs::remove_file(&manager.path).unwrap();
        fs::create_dir(&manager.path).unwrap();
        assert!(manager.stop(&record.id, &sessions, app.handle()).is_err());
        assert!(sessions.get(&record.id).is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn closed_runtime_admission_rejects_acp_start_before_spawning() {
        let (root, manager, sessions, record, app) = fixture();
        let gate = crate::runtime_admission::RuntimeAdmission::default();
        app.manage(gate.clone());
        assert!(manager.start(app.app_handle().clone(), &sessions, &record).unwrap().connected);
        manager.stop(&record.id, &sessions, app.app_handle()).unwrap();
        gate.begin_shutdown(Duration::from_secs(1)).unwrap().commit();
        assert!(manager.start(app.app_handle().clone(), &sessions, &record).is_err());
        let live = manager.live_session(&record.id).unwrap();
        assert_eq!(live.process_id.load(Ordering::Acquire), 0);
        assert!(live.connection.lock().unwrap().is_none());
        assert!(!live.connected.load(Ordering::Acquire));
        fs::remove_dir_all(root).unwrap();
    }

    fn wait_for(manager: &NativeAcpSessions, id: &str, predicate: impl Fn(&HarnessSessionSnapshot) -> bool) -> HarnessSessionSnapshot {
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = manager.snapshot(id).unwrap();
            if predicate(&snapshot) { return snapshot; }
            assert!(std::time::Instant::now() < deadline, "ACP state did not settle: {snapshot:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn kimi_fixture() -> (PathBuf, NativeAcpSessions, SessionManager, SessionRecord, tauri::App<tauri::test::MockRuntime>) {
        let root = std::env::temp_dir().join(format!("cc-kimi-chat-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sessions = SessionManager::open_at(root.join("records.json"), root.join("events")).unwrap();
        let record = sessions.create_runtime(crate::sessions::NewRuntimeSession {
            runtime_kind: "kimi_chat".into(), cwd: root.display().to_string(), project_id: None,
        }).unwrap();
        let mut manager = NativeAcpSessions::open_at_flavor(root.join("kimi.json"), NativeChatFlavor::KimiChat).unwrap();
        manager.test_peer = Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/kimi-acp-agent.mjs"));
        manager.register(&record.id, &record.cwd).unwrap();
        (root, manager, sessions, record, tauri::test::mock_app())
    }

    fn opencode_fixture() -> (PathBuf, NativeAcpSessions, SessionManager, SessionRecord, tauri::App<tauri::test::MockRuntime>) {
        let root = std::env::temp_dir().join(format!("cc-opencode-chat-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sessions = SessionManager::open_at(root.join("records.json"), root.join("events")).unwrap();
        let record = sessions.create_runtime(crate::sessions::NewRuntimeSession {
            runtime_kind: "opencode_chat".into(), cwd: root.display().to_string(), project_id: None,
        }).unwrap();
        let mut manager = NativeAcpSessions::open_at_flavor(root.join("opencode.json"), NativeChatFlavor::OpenCodeChat).unwrap();
        manager.test_peer = Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/opencode-acp-agent.mjs"));
        manager.register(&record.id, &record.cwd).unwrap();
        (root, manager, sessions, record, tauri::test::mock_app())
    }

    #[test]
    fn opencode_chat_streams_approves_commands_and_config_options() {
        let (root, manager, sessions, record, app) = opencode_fixture();
        let first = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(first.connected);
        let config_ids = first.config_options.iter().map(|option| option["id"].as_str().unwrap_or_default()).collect::<Vec<_>>();
        assert_eq!(config_ids, ["model", "effort", "mode"]);
        // opencode 的 configOptions 三选择器（model/effort/mode）都走通用 set_config。
        manager.set_config(&record.id, "model", "opencode-alt").unwrap();
        manager.set_config(&record.id, "mode", "plan").unwrap();
        // set_config 之后的 available_commands_update 落进快照命令面板。
        let with_commands = wait_for(&manager, &record.id, |s| !s.commands.is_empty());
        assert!(with_commands.commands.iter().any(|command| command["name"].as_str() == Some("/compact")));
        let configured = manager.snapshot(&record.id).unwrap();
        assert_eq!(configured.config_options[0]["currentValue"], "opencode-alt");
        assert_eq!(configured.config_options[2]["currentValue"], "plan");
        manager.send_message(app.handle().clone(), &sessions, &record.id, "检查项目").unwrap();
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        assert!(pending.busy);
        assert!(pending.history.iter().any(|m| m.role == "assistant" && m.text.starts_with("流式回复")));
        assert!(pending.history.iter().any(|m| m.role == "assistant" && !m.reasoning.is_empty()));
        let permission = pending.pending_permission.unwrap();
        assert_eq!(permission["toolCall"]["title"], "读取文件");
        manager.answer_permission(&record.id, permission["requestId"].as_str().unwrap(), true).unwrap();
        let done = wait_for(&manager, &record.id, |s| !s.busy && s.pending_permission.is_none());
        assert_eq!(done.tool_calls[0]["status"], "completed");
        assert!(done.history.last().unwrap().text.ends_with("批准后的结尾。"));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opencode_chat_resume_drops_replayed_history_and_keeps_session_id() {
        let (root, manager, sessions, record, app) = opencode_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "第一轮").unwrap();
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        manager.answer_permission(&record.id, pending.pending_permission.unwrap()["requestId"].as_str().unwrap(), true).unwrap();
        let done = wait_for(&manager, &record.id, |s| !s.busy);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        // opencode 的 session/load 会在响应前回放历史通知；回放护栏应丢弃，本地历史不重复。
        let resumed = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(resumed.connected);
        assert_eq!(manager.locator(&record.id).unwrap().acp_session_id.as_deref(), Some("opencode-fake"));
        assert_eq!(resumed.history, done.history);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(manager.snapshot(&record.id).unwrap().history, done.history);
        // 回放护栏在首次发送后解除，新回合正常落库。
        manager.send_message(app.handle().clone(), &sessions, &record.id, "第二轮").unwrap();
        let second = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        manager.answer_permission(&record.id, second.pending_permission.unwrap()["requestId"].as_str().unwrap(), false).unwrap();
        let rejected = wait_for(&manager, &record.id, |s| !s.busy);
        assert_eq!(rejected.history.len(), done.history.len() + 2);
        assert!(rejected.history.last().unwrap().text.ends_with("已被拒绝。"));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn opencode_chat_reports_terminal_stderr_failure_without_waiting_for_acp_timeout() {
        let (root, manager, sessions, record, app) = opencode_fixture();
        let first = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "fixture free limit").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while sessions.get(&record.id).unwrap().status != SessionStatus::Error && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let failed = sessions.get(&record.id).unwrap();
        let snapshot = manager.snapshot(&record.id).unwrap();
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
        assert_eq!(failed.status, SessionStatus::Error, "terminal error must not hang until 30-minute timeout");
        let error = failed.last_error.unwrap();
        assert!(error.contains("免费额度"), "{error}");
        assert!(!error.contains("DO_NOT_DISPLAY") && !error.contains("apiKey"));
        assert!(!snapshot.busy && !snapshot.connected);
        assert_eq!(snapshot.config_options, first.config_options, "never change the model after a provider rejection");
    }

    #[test]
    fn opencode_chat_cancel_interrupts_inflight_turn() {
        let (root, manager, sessions, record, app) = opencode_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "cancel").unwrap();
        wait_for(&manager, &record.id, |s| s.busy);
        manager.cancel_turn(&record.id).unwrap();
        let cancelled = wait_for(&manager, &record.id, |s| !s.busy && !s.connected);
        assert!(cancelled.pending_permission.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn kimi_chat_streams_approves_commands_and_config_options() {
        let (root, manager, sessions, record, app) = kimi_fixture();
        let first = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(first.connected);
        let config_ids = first.config_options.iter().map(|option| option["id"].as_str().unwrap_or_default()).collect::<Vec<_>>();
        assert_eq!(config_ids, ["model", "thinking", "mode"]);
        // kimi 的 configOptions 三选择器（model/thinking/mode）都走通用 set_config。
        manager.set_config(&record.id, "model", "kimi-alt").unwrap();
        manager.set_config(&record.id, "mode", "plan").unwrap();
        // set_config 之后的 available_commands_update 落进快照命令面板。
        let with_commands = wait_for(&manager, &record.id, |s| !s.commands.is_empty());
        assert!(with_commands.commands.iter().any(|command| command["name"].as_str() == Some("/compact")));
        let configured = manager.snapshot(&record.id).unwrap();
        assert_eq!(configured.config_options[0]["currentValue"], "kimi-alt");
        assert_eq!(configured.config_options[2]["currentValue"], "plan");
        manager.send_message(app.handle().clone(), &sessions, &record.id, "检查项目").unwrap();
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        assert!(pending.busy);
        assert!(pending.history.iter().any(|m| m.role == "assistant" && m.text.starts_with("流式回复")));
        let permission = pending.pending_permission.unwrap();
        assert_eq!(permission["toolCall"]["title"], "读取文件");
        manager.answer_permission(&record.id, permission["requestId"].as_str().unwrap(), true).unwrap();
        let done = wait_for(&manager, &record.id, |s| !s.busy && s.pending_permission.is_none());
        assert_eq!(done.tool_calls[0]["status"], "completed");
        assert!(done.history.last().unwrap().text.ends_with("批准后的结尾。"));
        assert_eq!(sessions.get(&record.id).unwrap().title, record.title);
        let titles = sessions.drain_summary_requests();
        assert_eq!(titles.len(), 1, "replaceable metadata must not suppress the actual summary task");
        assert_eq!(titles[0].runtime_kind.as_deref(), Some("kimi_chat"));
        assert_eq!(titles[0].model.as_deref(), Some("kimi-alt"));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn kimi_chat_resume_drops_replayed_history_and_keeps_session_id() {
        let (root, manager, sessions, record, app) = kimi_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "第一轮").unwrap();
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        manager.answer_permission(&record.id, pending.pending_permission.unwrap()["requestId"].as_str().unwrap(), true).unwrap();
        let done = wait_for(&manager, &record.id, |s| !s.busy);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        // kimi 的 session/load 会在响应前回放历史通知；回放护栏应丢弃，本地历史不重复。
        let resumed = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(resumed.connected);
        assert_eq!(manager.locator(&record.id).unwrap().acp_session_id.as_deref(), Some("kimi-fake"));
        assert_eq!(resumed.history, done.history);
        std::thread::sleep(Duration::from_millis(400));
        assert_eq!(manager.snapshot(&record.id).unwrap().history, done.history);
        // 回放护栏在首次发送后解除，新回合正常落库。
        manager.send_message(app.handle().clone(), &sessions, &record.id, "第二轮").unwrap();
        let second = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        manager.answer_permission(&record.id, second.pending_permission.unwrap()["requestId"].as_str().unwrap(), false).unwrap();
        let rejected = wait_for(&manager, &record.id, |s| !s.busy);
        assert_eq!(rejected.history.len(), done.history.len() + 2);
        assert!(rejected.history.last().unwrap().text.ends_with("已被拒绝。"));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn kimi_chat_cancel_interrupts_inflight_turn() {
        let (root, manager, sessions, record, app) = kimi_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "cancel").unwrap();
        wait_for(&manager, &record.id, |s| s.busy);
        manager.cancel_turn(&record.id).unwrap();
        let cancelled = wait_for(&manager, &record.id, |s| !s.busy && !s.connected);
        assert!(cancelled.pending_permission.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_conversation_streams_merges_tools_approves_and_resumes_history() {
        let (root, manager, sessions, record, app) = fixture();
        let first = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(first.connected);
        manager.set_model(&record.id, "model-b").unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "检查项目").unwrap();
        assert!(manager.send_message(app.handle().clone(), &sessions, &record.id, "重复").is_err());
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        assert!(pending.busy);
        assert!(pending.history.iter().any(|m| m.role == "assistant" && m.text.starts_with("流式回复")));
        assert_eq!(pending.config_options[0]["currentValue"], "model-b");
        let permission = pending.pending_permission.unwrap();
        assert_eq!(permission["toolCall"]["title"], "读取项目文件");
        manager.answer_permission(&record.id, permission["requestId"].as_str().unwrap(), true).unwrap();
        let done = wait_for(&manager, &record.id, |s| !s.busy && s.pending_permission.is_none());
        assert_eq!(done.tool_calls[0]["title"], "读取项目文件");
        assert_eq!(done.tool_calls[0]["status"], "completed");
        assert!(done.history.last().unwrap().text.ends_with("批准完成"));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        let reopened = NativeAcpSessions::open_at(root.join("harness.json")).unwrap();
        assert_eq!(reopened.snapshot(&record.id).unwrap().history, done.history);
        let resumed = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(resumed.connected);
        assert_eq!(manager.locator(&record.id).unwrap().acp_session_id.as_deref(), Some("native-fake"));
        assert_eq!(resumed.history, done.history);
        manager.archive(&record.id, &sessions, app.handle()).unwrap();
        assert!(manager.start(app.handle().clone(), &sessions, &record).is_err());
        assert!(sessions.get(&record.id).unwrap().archived_at.is_some());
        manager.delete_with(&record.id, || sessions.delete(&record.id)).unwrap();
        assert!(!manager.contains(&record.id));
        assert!(sessions.get(&record.id).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn file_links_are_validated_sent_and_preserved_with_legacy_history() {
        let (root, manager, sessions, record, app) = fixture();
        let file = root.join("含空格 # file.bin");
        fs::write(&file, [0, 255, 0, 128]).unwrap();
        let input = file.to_str().unwrap().to_string();
        let links = normalize_attachments(&[input.clone(), input.clone()]).unwrap();
        assert_eq!(links.len(), 1);
        assert!(links[0].uri.starts_with("file:"));
        assert!(links[0].uri.contains("%23"));
        assert!(normalize_attachments(&[root.to_str().unwrap().into()]).is_err());
        assert!(normalize_attachments(&[root.join("missing").to_str().unwrap().into()]).is_err());
        assert!(normalize_attachments(&vec![input.clone(); 17]).is_err());
        let old: HarnessChatMessage = serde_json::from_value(json!({"id":"old","role":"user","text":"old","createdAt":1})).unwrap();
        assert!(old.attachments.is_empty());
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message_with_attachments(app.handle().clone(), &sessions, &record.id, "attachment-check", &[input]).unwrap();
        let done = wait_for(&manager, &record.id, |snapshot| !snapshot.busy);
        assert_eq!(done.history[0].attachments, links);
        assert!(done.history.last().unwrap().text.starts_with("附件已收到："));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        assert_eq!(NativeAcpSessions::open_at(root.join("harness.json")).unwrap().snapshot(&record.id).unwrap().history[0].attachments, links);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_native_stop_preserves_connection_process_and_session() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        let live = manager.live_session(&record.id).unwrap();
        let pid = live.process_id.load(Ordering::Acquire);
        let error = manager.stop_or_archive_with(&record.id, &sessions, app.handle(), false, |_| Err(anyhow!("fixture tree kill denied"))).unwrap_err();
        assert!(error.to_string().contains("kill denied"));
        assert_eq!(live.process_id.load(Ordering::Acquire), pid);
        assert!(live.connection.lock().unwrap().is_some());
        assert!(live.connected.load(Ordering::Acquire));
        assert_ne!(sessions.get(&record.id).unwrap().status, SessionStatus::Stopped);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stream_disconnect_reports_error_and_releases_connection_for_resume() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "disconnect-stream").unwrap();
        wait_for(&manager, &record.id, |s| !s.connected && !s.busy);
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Error);
        assert!(sessions.get(&record.id).unwrap().last_error.unwrap().contains("实时流"));
        let resumed = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(resumed.connected && !resumed.busy);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reasoning_and_answer_are_visible_before_completion_without_durable_duplicates() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "reasoning-stream").unwrap();
        let thinking = wait_for(&manager, &record.id, |s| s.history.iter().any(|m| m.reasoning == "真实测试思考首段"));
        assert!(thinking.busy);
        let reasoning = thinking.history.last().unwrap();
        assert!(reasoning.text.is_empty());
        assert_eq!(reasoning.stream_status, "streaming");
        let answering = wait_for(&manager, &record.id, |s| s.history.last().is_some_and(|m| m.text == "回答首段"));
        assert!(answering.busy);
        let done = wait_for(&manager, &record.id, |s| !s.busy);
        let assistants: Vec<_> = done.history.iter().filter(|m| m.role == "assistant").collect();
        assert_eq!(assistants.len(), 1);
        assert_eq!(assistants[0].text, "回答首段，第二段");
        assert_eq!(assistants[0].reasoning, "真实测试思考首段");
        assert_eq!(assistants[0].stream_status, "completed");
        manager.send_message(app.handle().clone(), &sessions, &record.id, "reasoning-stream").unwrap();
        wait_for(&manager, &record.id, |s| s.history.last().is_some_and(|m| m.reasoning == "真实测试思考首段" && m.stream_status == "streaming"));
        manager.cancel_turn(&record.id).unwrap();
        let cancelled = wait_for(&manager, &record.id, |s| !s.busy && !s.connected);
        assert_eq!(cancelled.history.last().unwrap().reasoning, "真实测试思考首段");
        assert_eq!(cancelled.history.last().unwrap().stream_status, "interrupted");
        std::thread::sleep(Duration::from_millis(300));
        let stopped = sessions.get(&record.id).unwrap();
        assert_eq!(stopped.status, SessionStatus::Stopped);
        assert!(stopped.last_error.is_none(), "normal cancellation reported stream failure: {:?}", stopped.last_error);
        assert!(manager.live_session(&record.id).unwrap().stream_failure.lock().unwrap().is_none());

        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        assert_eq!(NativeAcpSessions::open_at(root.join("harness.json")).unwrap().snapshot(&record.id).unwrap().history, cancelled.history);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skill_invocations_validate_before_history_and_preserve_arguments_and_files() {
        let (root, manager, sessions, record, app) = fixture();
        let started = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert_eq!(started.control_state["skills"][0]["name"], "demo-skill");
        for rejected in ["/unknown", "/goal new", "/plan", "/hidden"] {
            assert!(manager.send_message(app.handle().clone(), &sessions, &record.id, rejected).is_err());
            assert!(!manager.snapshot(&record.id).unwrap().busy);
        }
        assert!(manager.snapshot(&record.id).unwrap().history.is_empty());
        let file = root.join("skill-file.txt");
        fs::write(&file, "test").unwrap();
        manager.send_message_with_attachments(app.handle().clone(), &sessions, &record.id, "/demo-skill 中文参数", &[file.to_str().unwrap().into()]).unwrap();
        let done = wait_for(&manager, &record.id, |snapshot| !snapshot.busy);
        assert_eq!(done.history[0].text, "/demo-skill 中文参数");
        assert_eq!(done.history[0].attachments.len(), 1);
        assert_eq!(done.history.last().unwrap().text, "技能参数和附件已收到");
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_commands_use_control_plane_and_receive_autonomous_updates() {
        let (root, manager, sessions, record, app) = fixture();
        let snapshot = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert_eq!(snapshot.control_state["commands"][0]["name"], "goal");
        assert!(manager.send_message(app.handle().clone(), &sessions, &record.id, "/unknown").is_err());
        assert!(manager.control(app.handle(), &record.id, "command", "/unknown").is_err());
        assert!(manager.snapshot(&record.id).unwrap().history.is_empty());
        manager.control(app.handle(), &record.id, "permission", "read-only").unwrap();
        assert_eq!(manager.snapshot(&record.id).unwrap().control_state["permission"]["currentValue"], "read-only");
        assert!(manager.control(app.handle(), &record.id, "permission", "auto").is_err());
        let started = manager.control(app.handle(), &record.id, "command", "/goal 检查中文目标").unwrap();
        assert!(started.busy);
        assert_eq!(started.control_state["goal"]["objective"], "检查中文目标");
        wait_for(&manager, &record.id, |state| state.history.iter().any(|message| message.text.contains("自主目标回复")));
        let paused = manager.control(app.handle(), &record.id, "command", "/goal pause").unwrap();
        assert!(!paused.busy);
        assert_eq!(paused.control_state["goal"]["phase"], "paused");
        manager.control(app.handle(), &record.id, "command", "/goal clear").unwrap();
        manager.control(app.handle(), &record.id, "command", "/goal 取消检查").unwrap();
        assert!(manager.cancel_turn(&record.id).unwrap());
        let cancelled = wait_for(&manager, &record.id, |state| !state.busy && state.control_state["goal"]["phase"] == "paused");
        assert_eq!(cancelled.control_state["agentStatus"], "idle");
        let compact = manager.control(app.handle(), &record.id, "command", "/compact").unwrap();
        assert_eq!(compact.control_state["execution"]["result"]["text"], "No compactable history yet.");
        assert!(compact.history.iter().all(|message| message.role != "user"));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "installed Harness control-plugin smoke; no prompt, active goal, compact, or model requests"]
    fn installed_native_control_plugin_without_model_calls() {
        let (root, mut manager, sessions, record, app) = fixture();
        manager.test_peer = None;
        let started = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(started.control_state["commands"].as_array().unwrap().iter().any(|command| command["name"] == "goal"));
        assert_eq!(started.control_state["agentStatus"], "idle");
        assert!(started.control_state["skills"].as_array().unwrap().iter().all(|skill| skill["invocation"]["userInvocable"] == true));
        let skills = started.control_state["skills"].as_array().unwrap();
        assert!(!skills.is_empty(), "installed Harness should discover local user skills");
        println!("Harness discovered {} user-invocable skills; first: {}", skills.len(), skills[0]["name"]);
        manager.control(app.handle(), &record.id, "command", "/goal").unwrap();
        manager.control(app.handle(), &record.id, "permission", "read-only").unwrap();
        manager.control(app.handle(), &record.id, "permission", "workspace-write").unwrap();
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let resumed = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert_eq!(resumed.control_state["permission"]["currentValue"], "workspace-write");
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn legacy_verbatim_locator_preserves_existing_session_and_normalizes_unstarted_session() {
        let (root, manager, sessions, mut record, app) = fixture();
        let normalized = canonical_directory(root.to_str().unwrap()).unwrap();
        assert_eq!(manager.locator(&record.id).unwrap().cwd, normalized);
        let first = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(first.connected);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let legacy = root.canonicalize().unwrap().to_str().unwrap().to_string();
        assert!(legacy.starts_with(r"\\?\"));
        record.cwd = legacy.clone();
        manager.mutate_store(|store| {
            let entry = store.sessions.get_mut(&record.id).unwrap();
            entry.cwd = legacy.clone();
            entry.history.push(HarnessChatMessage {
                id: "old-message".into(), role: "user".into(), text: "保留原消息".into(), created_at: 1, attachments: Vec::new(), reasoning: String::new(), stream_status: String::new(),
            });
            Ok(())
        }).unwrap();
        let mut reopened = NativeAcpSessions::open_at(root.join("harness.json")).unwrap();
        reopened.test_peer = manager.test_peer.clone();
        // A different directory and permission must still be rejected.
        let other = root.join("other");
        fs::create_dir(&other).unwrap();
        let mut wrong_record = record.clone();
        wrong_record.cwd = other.display().to_string();
        assert!(reopened.start(app.handle().clone(), &sessions, &wrong_record).is_err());
        reopened.mutate_store(|store| {
            store.sessions.get_mut(&record.id).unwrap().permission_mode = "danger-full-access".into();
            Ok(())
        }).unwrap();
        assert!(reopened.start(app.handle().clone(), &sessions, &record).is_err());
        reopened.mutate_store(|store| {
            store.sessions.get_mut(&record.id).unwrap().permission_mode = "workspace-write".into();
            Ok(())
        }).unwrap();
        let before = fs::read(root.join("harness.json")).unwrap();
        let error = reopened.start(app.handle().clone(), &sessions, &record).unwrap_err();
        assert!(error.to_string().contains("原消息历史保留"));
        assert_eq!(fs::read(root.join("harness.json")).unwrap(), before);
        let preserved = reopened.locator(&record.id).unwrap();
        assert_eq!(preserved.cwd, legacy);
        assert_eq!(preserved.acp_session_id.as_deref(), Some("native-fake"));
        assert_eq!(preserved.history[0].text, "保留原消息");
        assert!(!reopened.snapshot(&record.id).unwrap().connected);
        // A local record without an upstream session can safely create a new
        // session using the normalized cwd.
        reopened.mutate_store(|store| {
            store.sessions.get_mut(&record.id).unwrap().acp_session_id = None;
            Ok(())
        }).unwrap();
        assert!(reopened.start(app.handle().clone(), &sessions, &record).unwrap().connected);
        let locator = reopened.locator(&record.id).unwrap();
        assert_eq!(locator.cwd, normalized);
        assert_eq!(locator.acp_session_id.as_deref(), Some("native-fake"));
        assert_eq!(locator.permission_mode, "workspace-write");
        reopened.stop_all(&sessions, app.handle()).unwrap();
        assert_eq!(NativeAcpSessions::open_at(root.join("harness.json")).unwrap().locator(&record.id).unwrap().cwd, normalized);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancellation_during_approval_and_stop_do_not_resurrect_old_turns() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "审批取消").unwrap();
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        manager.cancel_turn(&record.id).unwrap();
        let cancelled = wait_for(&manager, &record.id, |s| !s.busy && !s.connected);
        assert!(cancelled.pending_permission.is_none());
        assert!(manager.answer_permission(&record.id, pending.pending_permission.unwrap()["requestId"].as_str().unwrap(), true).is_err());
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "slow-stream").unwrap();
        let streaming = wait_for(&manager, &record.id, |s| s.history.last().is_some_and(|m| m.text == "首段"));
        assert!(streaming.busy && streaming.pending_permission.is_none());
        manager.cancel_turn(&record.id).unwrap();
        wait_for(&manager, &record.id, |s| !s.busy && !s.connected);
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "cancel").unwrap();
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let restarted = manager.snapshot(&record.id).unwrap();
        assert!(restarted.connected && !restarted.busy);
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
        manager.stop_all(&sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dead_idle_peer_releases_connection_and_disabled_runtime_cannot_resume() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        terminate_acp_process(&manager.live_session(&record.id).unwrap().process_id);
        wait_for(&manager, &record.id, |s| !s.connected);
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Error);
        crate::runtime_preferences::with_disabled_for_test(crate::multi_agent::AgentRuntimeKind::DeepSeekHarness, || {
            assert!(manager.start(app.handle().clone(), &sessions, &record).is_err());
        });
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        manager.stop_all(&sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn concurrent_starts_share_one_connection_and_delete_rollback_keeps_history() {
        let (root, manager, sessions, record, app) = fixture();
        let app_handle = app.handle().clone();
        std::thread::scope(|scope| {
            let first = scope.spawn(|| manager.start(app_handle.clone(), &sessions, &record).unwrap());
            let second = scope.spawn(|| manager.start(app_handle.clone(), &sessions, &record).unwrap());
            assert!(first.join().unwrap().connected);
            assert!(second.join().unwrap().connected);
        });
        assert_eq!(manager.snapshot(&record.id).unwrap().sequence, 1, "only one start publishes a running state");
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let before = fs::read(root.join("harness.json")).unwrap();
        assert!(manager.delete_with(&record.id, || Err(anyhow!("record deletion failed"))).is_err());
        assert_eq!(fs::read(root.join("harness.json")).unwrap(), before);
        assert!(manager.contains(&record.id));
        manager.delete_with(&record.id, || sessions.delete(&record.id)).unwrap();
        // A legacy failed launch without a locator remains deletable as well.
        let legacy = sessions.create_runtime(crate::sessions::NewRuntimeSession {
            runtime_kind: "deepseek_harness".into(), cwd: root.display().to_string(), project_id: None,
        }).unwrap();
        manager.delete_with(&legacy.id, || sessions.delete(&legacy.id)).unwrap();
        assert!(sessions.get(&legacy.id).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stopping_during_handshake_cancels_start_without_publishing_running() {
        let (root, mut manager, sessions, record, app) = fixture();
        let slow_peer = root.join("slow-peer.mjs");
        let peer = manager.test_peer.as_ref().unwrap().canonicalize().unwrap().display().to_string();
        let peer = peer.trim_start_matches("\\\\?\\").replace('\\', "/");
        fs::write(&slow_peer, format!("await new Promise(r=>setTimeout(r,500));await import({});", serde_json::to_string(&format!("file:///{peer}")).unwrap())).unwrap();
        manager.test_peer = Some(slow_peer);
        let live = manager.live_session(&record.id).unwrap();
        let app_handle = app.handle().clone();
        std::thread::scope(|scope| {
            let start = scope.spawn(|| manager.start(app_handle.clone(), &sessions, &record));
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while live.process_id.load(Ordering::Acquire) == 0 {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(5));
            }
            manager.stop(&record.id, &sessions, app.handle()).unwrap();
            assert!(start.join().unwrap().is_err());
        });
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Stopped);
        assert_eq!(live.process_id.load(Ordering::Acquire), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn store_does_not_publish_a_change_when_persistence_fails() {
        let root = std::env::temp_dir().join(format!("crewvela-native-acp-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let blocker = root.join("file");
        fs::create_dir(&blocker).unwrap();
        let manager = NativeAcpSessions::open_at(blocker.join("sessions.json")).unwrap();
        // Fail the mutation's write, not the initial history read (ENOTDIR on Unix).
        fs::remove_dir(&blocker).unwrap();
        fs::write(&blocker, "not a directory").unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        assert!(manager.register(&session_id, &root.display().to_string()).is_err());
        assert!(!manager.contains(&session_id));
        assert_eq!(fs::read_to_string(&blocker).unwrap(), "not a directory");
        let _ = fs::remove_dir_all(root);
    }

        fn claude_fixture() -> (PathBuf, NativeAcpSessions, SessionManager, SessionRecord, tauri::App<tauri::test::MockRuntime>) {
        let root = std::env::temp_dir().join(format!("cc-native-claude-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sessions = SessionManager::open_at(root.join("records.json"), root.join("events")).unwrap();
        let record = sessions.create_runtime(crate::sessions::NewRuntimeSession {
            runtime_kind: "claude_chat".into(), cwd: root.display().to_string(), project_id: None,
        }).unwrap();
        let mut manager = NativeAcpSessions::open_at_flavor(root.join("claude.json"), NativeChatFlavor::ClaudeChat).unwrap();
        manager.test_peer = Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/native-claude-acp.mjs"));
        manager.register(&record.id, &record.cwd).unwrap();
        (root, manager, sessions, record, tauri::test::mock_app())
    }

    #[test]
    fn claude_chat_streams_permissions_config_commands_and_replay_guard() {
        let (root, manager, sessions, record, app) = claude_fixture();
        let started = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(started.connected);
        assert!(started.commands.iter().any(|command| command["name"] == "clear"), "available commands should include clear: {:?}", started.commands);
        assert!(started.config_options.iter().any(|option| option["id"] == "mode"), "adapter config options should include mode: {:?}", started.config_options);
        manager.set_config(&record.id, "model", "opus").unwrap();
        assert_eq!(manager.snapshot(&record.id).unwrap().config_options[0]["currentValue"], "opus");
        manager.send_message(app.handle().clone(), &sessions, &record.id, "检查项目").unwrap();
        assert!(manager.send_message(app.handle().clone(), &sessions, &record.id, "again").is_err());
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        assert!(pending.busy);
        assert!(pending.history.iter().any(|m| m.role == "assistant" && m.text.starts_with("流式回复")));
        let permission = pending.pending_permission.unwrap();
        assert_eq!(permission["toolCall"]["title"], "读取项目文件");
        manager.answer_permission(&record.id, permission["requestId"].as_str().unwrap(), true).unwrap();
        let done = wait_for(&manager, &record.id, |s| !s.busy && s.pending_permission.is_none());
        assert_eq!(done.tool_calls[0]["title"], "读取项目文件");
        assert_eq!(done.tool_calls[0]["status"], "completed");
        assert!(done.history.last().unwrap().text.contains("批准完成"));
        assert_eq!(sessions.drain_transitions().into_iter().filter(|event| event.to == SessionStatus::Completed).count(), 1);
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
        // 斜杠命令以普通 prompt 文本发送并由上游执行
        manager.send_message(app.handle().clone(), &sessions, &record.id, "/model opus").unwrap();
        wait_for(&manager, &record.id, |s| s.history.last().is_some_and(|m| m.text.contains("命令已执行")));
        // claude 分支没有控制插件
        assert!(manager.control(app.handle(), &record.id, "command", "/compact").is_err());
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        let persisted = manager.snapshot(&record.id).unwrap().history.clone();
        let mut reopened = NativeAcpSessions::open_at_flavor(root.join("claude.json"), NativeChatFlavor::ClaudeChat).unwrap();
        reopened.test_peer = manager.test_peer.clone();
        assert_eq!(reopened.snapshot(&record.id).unwrap().history, persisted);
        let resumed = reopened.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(resumed.connected && !resumed.busy);
        // Claude Code 每次 resume 都开启新的上游会话文件：允许替换链接
        assert_eq!(reopened.locator(&record.id).unwrap().acp_session_id.as_deref(), Some("claude-fake-2"));
        // 等过重放窗口：load 后重放的历史通知必须被护栏丢弃
        std::thread::sleep(Duration::from_millis(1600));
        let guarded = reopened.snapshot(&record.id).unwrap();
        assert_eq!(guarded.history, persisted);
        assert!(!guarded.tool_calls.iter().any(|tool| tool["toolCallId"] == "replayed"));
        reopened.archive(&record.id, &sessions, app.handle()).unwrap();
        assert!(reopened.start(app.handle().clone(), &sessions, &record).is_err());
        reopened.delete_with(&record.id, || sessions.delete(&record.id)).unwrap();
        assert!(!reopened.contains(&record.id));
        assert!(sessions.get(&record.id).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn acp_stream_flush_failure_is_failed_even_if_storage_recovers_before_prompt_result() {
        let (root, manager, sessions, record, app) = claude_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "check persistence failure").unwrap();
        let pending = wait_for(&manager, &record.id, |snapshot| snapshot.pending_permission.is_some());
        let live = manager.live_session(&record.id).unwrap();
        let saved = fs::read(&manager.path).unwrap();
        fs::remove_file(&manager.path).unwrap();
        fs::create_dir(&manager.path).unwrap();
        live.pending_delta.lock().unwrap().0 = "unsaved reply".into();
        assert!(manager.flush_active_delta(app.handle(), &live, &record.id).is_err());
        assert!(live.stream_failure.lock().unwrap().is_some());
        fs::remove_dir(&manager.path).unwrap();
        fs::write(&manager.path, saved).unwrap();
        manager.answer_permission(&record.id, pending.pending_permission.unwrap()["requestId"].as_str().unwrap(), true).unwrap();
        wait_for(&manager, &record.id, |snapshot| !snapshot.busy && !snapshot.connected);
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Error);
        let terminal = sessions.drain_transitions().into_iter().filter(|event| matches!(event.to, SessionStatus::Completed | SessionStatus::Error | SessionStatus::Stopped)).collect::<Vec<_>>();
        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].to, SessionStatus::Error);
        manager.start(app.handle().clone(), &sessions, &sessions.get(&record.id).unwrap()).unwrap();
        assert!(live.stream_failure.lock().unwrap().is_none());
        manager.send_message(app.handle().clone(), &sessions, &record.id, "retry after storage recovered").unwrap();
        let pending = wait_for(&manager, &record.id, |snapshot| snapshot.pending_permission.is_some());
        manager.answer_permission(&record.id, pending.pending_permission.unwrap()["requestId"].as_str().unwrap(), true).unwrap();
        wait_for(&manager, &record.id, |snapshot| !snapshot.busy && snapshot.connected);
        assert_eq!(sessions.drain_transitions().into_iter().filter(|event| event.to == SessionStatus::Completed).count(), 1, "new turn must not inherit a previous storage failure");
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn claude_chat_cancel_interrupts_turn_and_liveness_recovers() {
        let (root, manager, sessions, record, app) = claude_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "slow-stream").unwrap();
        let streaming = wait_for(&manager, &record.id, |s| s.history.iter().any(|m| m.text == "首段"));
        assert!(streaming.busy);
        manager.cancel_turn(&record.id).unwrap();
        let cancelled = wait_for(&manager, &record.id, |s| !s.busy && !s.connected);
        assert!(cancelled.history.iter().any(|m| m.text == "首段"));
        // 取消后可以重新恢复会话
        let resumed = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(resumed.connected && !resumed.busy);
        manager.stop_all(&sessions, app.handle()).unwrap();
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        fs::remove_dir_all(root).unwrap();
    }

    fn codex_fixture() -> (PathBuf, NativeAcpSessions, SessionManager, SessionRecord, tauri::App<tauri::test::MockRuntime>) {
        let root = std::env::temp_dir().join(format!("cc-native-codex-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sessions = SessionManager::open_at(root.join("records.json"), root.join("events")).unwrap();
        let record = sessions.create_runtime(crate::sessions::NewRuntimeSession {
            runtime_kind: "codex_chat".into(), cwd: root.display().to_string(), project_id: None,
        }).unwrap();
        let mut manager = NativeAcpSessions::open_at_flavor(root.join("codex.json"), NativeChatFlavor::CodexChat).unwrap();
        manager.test_peer = Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/codex-acp-agent.mjs"));
        manager.register(&record.id, &record.cwd).unwrap();
        (root, manager, sessions, record, tauri::test::mock_app())
    }

    #[test]
    fn codex_goal_completion_streams_and_persists_native_state() {
        let (root, manager, sessions, record, app) = codex_fixture();
        assert!(manager.start(app.handle().clone(), &sessions, &record).unwrap().codex_goal_supported);
        manager.control_codex_goal(app.handle().clone(), &record.id, "set", Some("complete")).unwrap();
        let done = wait_for(&manager, &record.id, |s| s.codex_goal.as_ref().is_some_and(|goal| goal.status == "complete") && s.codex_goal_action.is_none() && !s.busy);
        assert_eq!(done.codex_goal.as_ref().unwrap().tokens_used, 123);
        assert!(done.history.iter().any(|message| message.text.contains("真实目标协议")));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let reopened = NativeAcpSessions::open_at_flavor(root.join("codex.json"), NativeChatFlavor::CodexChat).unwrap();
        assert_eq!(reopened.snapshot(&record.id).unwrap().codex_goal, done.codex_goal);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_goal_pause_during_approval_and_clear_do_not_block_or_tear_down_connection() {
        let (root, manager, sessions, record, app) = codex_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.control_codex_goal(app.handle().clone(), &record.id, "set", Some("approval")).unwrap();
        wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        assert!(manager.send_message(app.handle().clone(), &sessions, &record.id, "并发任务").is_err());
        let began = std::time::Instant::now();
        manager.control_codex_goal(app.handle().clone(), &record.id, "pause", None).unwrap();
        assert!(began.elapsed() < Duration::from_secs(1), "pause must not wait behind approval or the goal's pending RPC");
        let paused = wait_for(&manager, &record.id, |s| s.codex_goal.as_ref().is_some_and(|goal| goal.status == "paused") && s.codex_goal_action.is_none() && !s.busy);
        assert!(paused.connected && paused.pending_permission.is_none());
        assert!(paused.history.iter().any(|message| message.text.contains("真实目标协议")));
        manager.control_codex_goal(app.handle().clone(), &record.id, "resume", None).unwrap();
        wait_for(&manager, &record.id, |s| s.codex_goal.as_ref().is_some_and(|goal| goal.status == "active") && s.pending_permission.is_some());
        manager.control_codex_goal(app.handle().clone(), &record.id, "clear", None).unwrap();
        let cleared = wait_for(&manager, &record.id, |s| s.codex_goal.is_none() && s.codex_goal_action.is_none() && !s.busy);
        assert!(cleared.connected && cleared.pending_permission.is_none());
        std::thread::sleep(Duration::from_millis(400));
        assert!(manager.snapshot(&record.id).unwrap().codex_goal.is_none(), "late earlier RPC reply must not restore a cleared goal");
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_goal_rejection_and_stop_release_pending_state_without_resurrection() {
        let (root, manager, sessions, record, app) = codex_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(manager.control_codex_goal(app.handle().clone(), &record.id, "set", Some(" ")).is_err());
        manager.control_codex_goal(app.handle().clone(), &record.id, "set", Some("fail")).unwrap();
        let rejected = wait_for(&manager, &record.id, |s| s.codex_goal_action.is_none() && !s.busy);
        assert!(rejected.codex_goal.is_none());
        manager.control_codex_goal(app.handle().clone(), &record.id, "set", Some("wait")).unwrap();
        wait_for(&manager, &record.id, |s| s.codex_goal.is_some());
        let began = std::time::Instant::now();
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        assert!(began.elapsed() < Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(400));
        let stopped = manager.snapshot(&record.id).unwrap();
        assert!(!stopped.connected && !stopped.busy && stopped.codex_goal_action.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_goal_turn_identity_preserves_current_failure_and_clears_it_for_next_turn() {
        let live = super::LiveSession::new();
        *live.stream_failure.lock().unwrap() = Some("previous turn failed".into());
        live.ensure_codex_goal_turn();
        assert!(live.stream_failure.lock().unwrap().is_none());
        let first = live.active_turn_id.lock().unwrap().clone().unwrap();
        *live.stream_failure.lock().unwrap() = Some("current turn failed".into());
        live.ensure_codex_goal_turn();
        assert_eq!(live.active_turn_id.lock().unwrap().as_deref(), Some(first.as_str()));
        assert_eq!(live.stream_failure.lock().unwrap().as_deref(), Some("current turn failed"));
        *live.active_turn_id.lock().unwrap() = None;
        live.ensure_codex_goal_turn();
        assert_ne!(live.active_turn_id.lock().unwrap().as_deref(), Some(first.as_str()));
        assert!(live.stream_failure.lock().unwrap().is_none());
    }

    #[test]
    fn codex_goal_ack_and_paused_metadata_do_not_end_native_turn() {
        let (root, manager, sessions, record, app) = codex_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.control_codex_goal(app.handle().clone(), &record.id, "set", Some("early-ack")).unwrap();
        let running = wait_for(&manager, &record.id, |s| s.codex_goal_action.is_none() && s.codex_goal.is_some());
        assert!(running.busy, "goal ACK is not a native idle notification");
        manager.control_codex_goal(app.handle().clone(), &record.id, "pause", None).unwrap();
        let paused_running = wait_for(&manager, &record.id, |s| s.codex_goal_action.is_none() && s.codex_goal.as_ref().is_some_and(|goal| goal.status == "paused"));
        assert!(paused_running.busy, "paused metadata must wait for native turn idle");
        assert!(manager.send_message(app.handle().clone(), &sessions, &record.id, "unsafe overlap").is_err());
        wait_for(&manager, &record.id, |s| !s.busy);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_goal_clear_does_not_reject_manual_prompt_approval() {
        let (root, manager, sessions, record, app) = codex_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.control_codex_goal(app.handle().clone(), &record.id, "set", Some("complete")).unwrap();
        wait_for(&manager, &record.id, |s| !s.busy && s.codex_goal_action.is_none());
        manager.send_message(app.handle().clone(), &sessions, &record.id, "ordinary task").unwrap();
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        let request = pending.pending_permission.unwrap()["requestId"].as_str().unwrap().to_string();
        manager.control_codex_goal(app.handle().clone(), &record.id, "clear", None).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        assert!(manager.snapshot(&record.id).unwrap().pending_permission.is_some());
        manager.answer_permission(&record.id, &request, true).unwrap();
        wait_for(&manager, &record.id, |s| !s.busy && s.codex_goal.is_none());
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_goal_cancel_is_scoped_to_turn_and_next_approval_survives() {
        let (root, manager, sessions, record, app) = codex_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.control_codex_goal(app.handle().clone(), &record.id, "set", Some("early-ack")).unwrap();
        wait_for(&manager, &record.id, |s| s.codex_goal.is_some() && s.codex_goal_action.is_none());
        manager.cancel_turn(&record.id).unwrap();
        wait_for(&manager, &record.id, |s| !s.busy);
        assert!(!manager.live_session(&record.id).unwrap().cancelled.load(Ordering::Acquire));
        manager.control_codex_goal(app.handle().clone(), &record.id, "pause", None).unwrap();
        wait_for(&manager, &record.id, |s| s.codex_goal_action.is_none());
        manager.control_codex_goal(app.handle().clone(), &record.id, "set", Some("approval")).unwrap();
        let next = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        manager.answer_permission(&record.id, next.pending_permission.unwrap()["requestId"].as_str().unwrap(), true).unwrap();
        wait_for(&manager, &record.id, |s| !s.busy && s.codex_goal.as_ref().is_some_and(|g| g.status == "complete"));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_goal_slash_commands_use_goal_lifecycle_instead_of_manual_prompt() {
        let (root, manager, sessions, record, app) = codex_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(manager.send_message(app.handle().clone(), &sessions, &record.id, "/goal").is_err());
        manager.send_message(app.handle().clone(), &sessions, &record.id, "/goal\ncomplete").unwrap();
        wait_for(&manager, &record.id, |s| !s.busy && s.codex_goal.as_ref().is_some_and(|g| g.status == "complete"));
        assert!(!manager.live_session(&record.id).unwrap().prompt_inflight.load(Ordering::Acquire));
        manager.send_message(app.handle().clone(), &sessions, &record.id, "/goal early-ack").unwrap();
        wait_for(&manager, &record.id, |s| s.busy && s.codex_goal_action.is_none());
        manager.send_message(app.handle().clone(), &sessions, &record.id, "/goal pause").unwrap();
        wait_for(&manager, &record.id, |s| !s.busy && s.codex_goal_action.is_none());
        manager.send_message(app.handle().clone(), &sessions, &record.id, "/goal clear").unwrap();
        wait_for(&manager, &record.id, |s| s.codex_goal.is_none() && s.codex_goal_action.is_none());
        assert!(manager.snapshot(&record.id).unwrap().connected);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_plugins_validate_server_catalog_before_sending_native_resource() {
        let (root, manager, sessions, record, app) = codex_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(available_codex_plugin_ids(&manager.codex_plugins(&record.id).unwrap()).contains("figma@test"));
        assert_eq!(manager.snapshot(&record.id).unwrap().context_usage.as_ref().unwrap().used, 1);
        let send_plugin = |plugin: &str| {
            let (sender, receiver) = mpsc::channel();
            let worker = manager.clone();
            let handle = app.handle().clone();
            let sessions = sessions.clone();
            let id = record.id.clone();
            let plugin = plugin.to_string();
            std::thread::spawn(move || { let _ = sender.send(worker.send_codex_message_with_plugins(handle, &sessions, &id, "with plugin", &[], &[plugin])); });
            receiver.recv_timeout(Duration::from_secs(5)).expect("catalog session/update must not deadlock the publication lock")
        };
        for plugin in ["disabled@test", "missing@test", "plugin://figma@test", "figma@test/../x"] {
            assert!(send_plugin(plugin).is_err());
            assert!(!manager.snapshot(&record.id).unwrap().busy);
        }
        send_plugin("figma@test").unwrap();
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        assert!(pending.history.iter().any(|m| m.text.contains("附件已收到")));
        manager.answer_permission(&record.id, pending.pending_permission.unwrap()["requestId"].as_str().unwrap(), true).unwrap();
        wait_for(&manager, &record.id, |s| !s.busy);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn codex_context_usage_roundtrips_stream_snapshot_and_persistence() {
        let (root, manager, sessions, record, app) = codex_fixture();
        assert!(manager.start(app.handle().clone(), &sessions, &record).unwrap().context_usage.is_none());
        manager.send_message(app.handle().clone(), &sessions, &record.id, "usage:normal").unwrap();
        let normal = wait_for(&manager, &record.id, |s| !s.busy);
        assert_eq!(normal.context_usage, Some(crate::acp::AcpContextUsage { used: 168000, size: 258000 }));
        manager.send_message(app.handle().clone(), &sessions, &record.id, "usage:compact").unwrap();
        let compact = wait_for(&manager, &record.id, |s| !s.busy);
        assert_eq!(compact.context_usage.as_ref().unwrap().used, 24000);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let mut reopened = NativeAcpSessions::open_at_flavor(root.join("codex.json"), NativeChatFlavor::CodexChat).unwrap();
        reopened.test_peer = manager.test_peer.clone();
        assert_eq!(reopened.snapshot(&record.id).unwrap().context_usage, compact.context_usage);
        reopened.start(app.handle().clone(), &sessions, &record).unwrap();
        assert_eq!(reopened.snapshot(&record.id).unwrap().context_usage, compact.context_usage);
        reopened.stop(&record.id, &sessions, app.handle()).unwrap();
        reopened.mutate_store(|store| {
            store.sessions.get_mut(&record.id).unwrap().acp_session_id = Some("invalid-usage".into());
            Ok(())
        }).unwrap();
        let invalid_resume = reopened.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(invalid_resume.context_usage.is_none(), "invalid load notification must clear previously persisted occupancy");
        reopened.set_config(&record.id, "model", "gpt-alt").unwrap();
        assert!(reopened.snapshot(&record.id).unwrap().context_usage.is_none());
        reopened.send_message(app.handle().clone(), &sessions, &record.id, "usage:invalid").unwrap();
        assert!(wait_for(&reopened, &record.id, |s| !s.busy).context_usage.is_none());
        reopened.stop(&record.id, &sessions, app.handle()).unwrap();
        let other = sessions.create_runtime(crate::sessions::NewRuntimeSession { runtime_kind: "codex_chat".into(), cwd: root.display().to_string(), project_id: None }).unwrap();
        reopened.register(&other.id, &other.cwd).unwrap();
        assert!(reopened.snapshot(&other.id).unwrap().context_usage.is_none());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn context_usage_rejects_invalid_numbers_without_using_cost() {
        for value in [json!({"used":-1,"size":100}), json!({"used":0,"size":0}), json!({"used":1.5,"size":100}), json!({"cost":{"amount":65}}), json!({"used":9007199254740992u64,"size":100})] {
            assert!(crate::acp::parse_context_usage(&value).is_none());
        }
        assert_eq!(crate::acp::parse_context_usage(&json!({"used":0,"size":100})), Some(crate::acp::AcpContextUsage { used: 0, size: 100 }));
        let old: SessionLocator = serde_json::from_value(json!({"cwd":"test","permissionMode":"agent","acpSessionId":null,"history":[],"configOptions":[]})).unwrap();
        assert!(old.context_usage.is_none());
    }

    #[test]
    fn codex_chat_streams_reasoning_permissions_config_and_resumes() {
        let (root, manager, sessions, record, app) = codex_fixture();
        let started = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(started.connected);
        assert!(started.commands.iter().any(|command| command["name"] == "clear"), "ensure_clear_command should inject clear: {:?}", started.commands);
        assert!(started.config_options.iter().any(|option| option["id"] == "model"), "codex adapter should expose model config: {:?}", started.config_options);
        manager.set_config(&record.id, "model", "gpt-alt").unwrap();
        assert_eq!(manager.snapshot(&record.id).unwrap().config_options[0]["currentValue"], "gpt-alt");
        manager.send_message(app.handle().clone(), &sessions, &record.id, "检查项目").unwrap();
        assert!(manager.send_message(app.handle().clone(), &sessions, &record.id, "重复").is_err());
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        assert!(pending.busy);
        // codex-acp 的思考流进入 reasoning，回答流进入 text
        assert!(pending.history.iter().any(|m| m.role == "assistant" && m.reasoning == "先查看相关文件。"));
        assert!(pending.history.iter().any(|m| m.role == "assistant" && m.text.starts_with("流式开头")));
        let permission = pending.pending_permission.unwrap();
        assert_eq!(permission["toolCall"]["title"], "读取文件");
        manager.answer_permission(&record.id, permission["requestId"].as_str().unwrap(), true).unwrap();
        let done = wait_for(&manager, &record.id, |s| !s.busy && s.pending_permission.is_none());
        assert_eq!(done.tool_calls[0]["title"], "读取文件");
        assert_eq!(done.tool_calls[0]["status"], "completed");
        assert!(done.history.last().unwrap().text.ends_with("，批准后的结尾。"));
        assert_eq!(done.history.last().unwrap().reasoning, "先查看相关文件。");
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        let persisted = manager.snapshot(&record.id).unwrap().history.clone();
        let mut reopened = NativeAcpSessions::open_at_flavor(root.join("codex.json"), NativeChatFlavor::CodexChat).unwrap();
        reopened.test_peer = manager.test_peer.clone();
        assert_eq!(reopened.snapshot(&record.id).unwrap().history, persisted);
        let resumed = reopened.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(resumed.connected && !resumed.busy);
        assert_eq!(reopened.locator(&record.id).unwrap().acp_session_id.as_deref(), Some("codex-fake"));
        reopened.stop(&record.id, &sessions, app.handle()).unwrap();
        // codex-acp 的 load 失败会回退 session/new：与本地历史错位时拒绝启动
        reopened.mutate_store(|store| {
            store.sessions.get_mut(&record.id).unwrap().acp_session_id = Some("missing".into());
            Ok(())
        }).unwrap();
        assert!(reopened.start(app.handle().clone(), &sessions, &record).unwrap_err().to_string().contains("回退到了新会话"));
        reopened.archive(&record.id, &sessions, app.handle()).unwrap();
        reopened.delete_with(&record.id, || sessions.delete(&record.id)).unwrap();
        assert!(!reopened.contains(&record.id));
        assert!(sessions.get(&record.id).is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_chat_cancel_interrupts_turn_and_denial_reports_failed_tool() {
        let (root, manager, sessions, record, app) = codex_fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.send_message(app.handle().clone(), &sessions, &record.id, "拒绝路径").unwrap();
        let pending = wait_for(&manager, &record.id, |s| s.pending_permission.is_some());
        manager.answer_permission(&record.id, pending.pending_permission.unwrap()["requestId"].as_str().unwrap(), false).unwrap();
        let denied = wait_for(&manager, &record.id, |s| !s.busy && s.pending_permission.is_none());
        assert_eq!(denied.tool_calls[0]["status"], "failed");
        assert!(denied.history.last().unwrap().text.ends_with("，已被拒绝。"));
        assert_eq!(sessions.drain_transitions().into_iter().filter(|event| event.to == SessionStatus::Completed).count(), 1);
        manager.send_message(app.handle().clone(), &sessions, &record.id, "cancel").unwrap();
        let streaming = wait_for(&manager, &record.id, |s| s.busy && s.history.iter().any(|m| m.role == "assistant" && m.text.starts_with("流式开头")));
        assert!(streaming.busy);
        manager.cancel_turn(&record.id).unwrap();
        let cancelled = wait_for(&manager, &record.id, |s| !s.busy && !s.connected);
        assert!(cancelled.history.iter().any(|m| m.text.contains("流式开头")));
        assert!(sessions.drain_transitions().iter().all(|event| !matches!(event.to, SessionStatus::Completed | SessionStatus::Error)), "cancelled ACP turn must not fire terminal automations");
        // 取消后可以重新恢复会话
        let resumed = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(resumed.connected && !resumed.busy);
        manager.stop_all(&sessions, app.handle()).unwrap();
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        fs::remove_dir_all(root).unwrap();
    }

#[test]
    fn persisted_locator_rejects_changed_directory_and_non_uuid_ids() {
        let root = std::env::temp_dir().join(format!("crewvela-native-acp-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let manager = NativeAcpSessions::open_at(root.join("sessions.json")).unwrap();
        let session_id = uuid::Uuid::new_v4().to_string();
        manager.register(&session_id, &root.display().to_string()).unwrap();
        assert!(manager.locator("not-a-session-id").is_err());
        assert_eq!(manager.locator(&session_id).unwrap().cwd, canonical_directory(root.to_str().unwrap()).unwrap());
        let _ = fs::remove_dir_all(root);
    }
}
