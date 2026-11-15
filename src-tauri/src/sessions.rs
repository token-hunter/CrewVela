use std::collections::{HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

use crate::worktree::SessionWorktree;

const MAX_TITLE_CHARS: usize = 25;
const SESSIONS_VERSION: u32 = 1;
const APP_SESSION_ID_FIELD: &str = crate::brand::APP_SESSION;

fn remove_ingested_event(path: &std::path::Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).with_context(|| format!("清理会话事件失败: {}", path.display())),
    }
}

fn transcript_session_id(event: &serde_json::Value) -> Option<&str> {
    event
        .get("transcript_path")
        .and_then(|value| value.as_str())
        .and_then(|path| path.rsplit(|ch| ch == '/' || ch == '\\').next())
        .and_then(|name| name.strip_suffix(".jsonl"))
        .filter(|session_id| !session_id.is_empty())
}

fn has_background_tasks(event: &serde_json::Value) -> bool {
    event
        .get("background_tasks")
        .and_then(|value| value.as_array())
        .is_some_and(|tasks| !tasks.is_empty())
}

fn event_summary(event: &serde_json::Value) -> Option<String> {
    ["last_assistant_message", "result", "message"]
        .into_iter()
        .find_map(|key| {
            event
                .get(key)
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn event_prompt(event: &serde_json::Value) -> Option<String> {
    event
        .get("input_prompt")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

pub fn title_from_prompt(prompt: &str) -> String {
    let normalized = prompt.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return "新会话".to_string();
    }

    let mut chars = normalized.chars();
    let title: String = chars.by_ref().take(MAX_TITLE_CHARS).collect();
    if chars.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}

pub fn queue_hook_event(
    events_dir: &std::path::Path,
    event: &serde_json::Value,
) -> Result<Option<serde_json::Value>> {
    fs::create_dir_all(events_dir)
        .with_context(|| format!("创建会话事件目录失败: {}", events_dir.display()))?;
    let queued_at = now_nanos();
    let event_id = uuid::Uuid::new_v4().to_string();
    let temp_path = events_dir.join(format!("{queued_at:020}-{event_id}.tmp"));
    let event_path = events_dir.join(format!("{queued_at:020}-{event_id}.json"));
    let mut queued_event = crate::brand::normalize_hook(event)?;
    if let Some(object) = queued_event.as_object_mut() {
        object.insert(
            "_crewvela_queued_at".into(),
            serde_json::Value::from(queued_at),
        );
    }
    let queued_event = crate::brand::compatible_hook(&queued_event)?;
    fs::write(&temp_path, serde_json::to_vec(&queued_event)?)
        .with_context(|| format!("写入会话事件失败: {}", temp_path.display()))?;
    fs::rename(&temp_path, &event_path)
        .with_context(|| format!("提交会话事件失败: {}", event_path.display()))?;

    let response = match (
        event
            .get("hook_event_name")
            .and_then(|value| value.as_str()),
        event.get("prompt").and_then(|value| value.as_str()),
    ) {
        (Some("UserPromptSubmit"), Some(prompt)) => Some(serde_json::json!({
            "hookSpecificOutput": {
                "hookEventName": "UserPromptSubmit",
                "sessionTitle": title_from_prompt(prompt)
            }
        })),
        _ => None,
    };
    Ok(response)
}

pub fn run_hook(events_dir: &std::path::Path, app_session_id: Option<&str>) -> Result<()> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("读取 Claude Hook 输入失败")?;
    let mut event: serde_json::Value =
        serde_json::from_str(&input).context("解析 Claude Hook 输入失败")?;
    if let (Some(app_session_id), Some(object)) = (app_session_id, event.as_object_mut()) {
        object.insert(
            APP_SESSION_ID_FIELD.into(),
            serde_json::Value::String(app_session_id.to_string()),
        );
    }
    enrich_stop_event_summary(&mut event);
    if let Some(response) = queue_hook_event(events_dir, &event)? {
        let mut stdout = std::io::stdout().lock();
        serde_json::to_writer(&mut stdout, &response)?;
        stdout.write_all(b"\n")?;
    }
    Ok(())
}

fn enrich_stop_event_summary(event: &mut serde_json::Value) {
    let is_stop = event
        .get("hook_event_name")
        .and_then(serde_json::Value::as_str)
        == Some("Stop");
    if !is_stop {
        return;
    }
    let Some(path) = event
        .get("transcript_path")
        .and_then(serde_json::Value::as_str)
    else {
        return;
    };
    let transcript_path = std::path::Path::new(path);
    let summary = last_message_from_transcript(transcript_path, "assistant");
    let prompt = last_message_from_transcript(transcript_path, "user");
    let has_summary = event_summary(event).is_some();
    if let Some(object) = event.as_object_mut() {
        if !has_summary {
            if let Some(summary) = summary {
                object.insert(
                    "last_assistant_message".into(),
                    serde_json::Value::String(summary),
                );
            }
        }
        if let Some(prompt) = prompt {
            object.insert("input_prompt".into(), serde_json::Value::String(prompt));
        }
    }
}

fn claude_pasted_transcript_text(text: &str) -> &str {
    // Claude CLI records bracketed paste with an outer transport tag, while
    // hook/transcript paths may retain different surrounding whitespace.
    // Remove exactly one matched outer
    // wrapper; keep the prompt itself intact for stale-turn identity checks.
    let text = text.trim();
    let Some((id, body)) = text.strip_prefix("<pasted_content id=\"")
        .and_then(|rest| rest.split_once("\">")) else { return text; };
    if id.is_empty() || id.len() > 64
        || !id.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
    { return text; }
    body.strip_suffix(&format!("</pasted_content id=\"{id}\">"))
        .or_else(|| body.strip_suffix("</pasted_content>"))
        .map(str::trim).unwrap_or(text)
}

fn last_message_from_transcript(path: &std::path::Path, role: &str) -> Option<String> {
    const MAX_TRANSCRIPT_TAIL: u64 = 2 * 1024 * 1024;
    let mut file = fs::File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let offset = length.saturating_sub(MAX_TRANSCRIPT_TAIL);
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    let raw = String::from_utf8_lossy(&bytes);
    raw.lines().rev().find_map(|line| {
        let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
        if value.get("type").and_then(serde_json::Value::as_str) != Some(role)
            || (role == "user"
                && value.get("isMeta").and_then(serde_json::Value::as_bool) == Some(true))
        {
            return None;
        }
        let content = value.get("message")?.get("content")?;
        let text = if let Some(text) = content.as_str() {
            text.trim().to_string()
        } else {
            content
                .as_array()?
                .iter()
                .filter(|item| item.get("type").and_then(serde_json::Value::as_str) == Some("text"))
                .filter_map(|item| item.get("text").and_then(serde_json::Value::as_str))
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let text = if role == "user" { claude_pasted_transcript_text(&text).to_owned() } else { text };
        (!text.is_empty()).then_some(text)
    })
}

pub fn run_agent_turn_hook(
    events_dir: &std::path::Path,
    app_session_id: &str,
    payload: &str,
) -> Result<()> {
    let notification: serde_json::Value =
        serde_json::from_str(payload).context("解析 Agent 完成通知失败")?;
    let event_type = notification
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    if event_type != "agent-turn-complete" {
        return Ok(());
    }
    let runtime_session_id = notification
        .get("thread-id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(app_session_id);
    let input_prompt = notification
        .get("input-messages")
        .and_then(serde_json::Value::as_array)
        .map(|messages| {
            messages
                .iter()
                .filter_map(serde_json::Value::as_str)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    let event = serde_json::json!({
        "session_id": runtime_session_id,
        "hook_event_name": "AgentTurnComplete",
        "last_assistant_message": notification
            .get("last-assistant-message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        "input_prompt": input_prompt,
        APP_SESSION_ID_FIELD: app_session_id,
    });
    queue_hook_event(events_dir, &event)?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Pending,
    Running,
    Waiting,
    Completed,
    Error,
    Stopped,
}

/// 一次「非人工」状态跃迁（hook 事件 / 进程退出 / 启动失败驱动）。
/// 自动化事件触发器（会话完成/失败时）的事件源；pty_poll_all 每轮 drain。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTransition {
    pub session_id: String,
    pub from: SessionStatus,
    pub to: SessionStatus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    #[default]
    Claude,
    Terminal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TerminalShell {
    Powershell,
    Cmd,
    Zsh,
    Bash,
}

impl TerminalShell {
    pub fn display_name(self) -> &'static str {
        match self {
            Self::Powershell => "PowerShell",
            Self::Cmd => "CMD",
            Self::Zsh => "Zsh",
            Self::Bash => "Bash",
        }
    }

    pub fn platform_default() -> Self {
        if cfg!(target_os = "windows") { Self::Powershell }
        else if cfg!(target_os = "macos") { Self::Zsh }
        else { Self::Bash }
    }

    pub fn agent_host_default() -> Self {
        if cfg!(windows) { Self::Powershell } else { Self::Bash }
    }

    pub fn executable(self) -> &'static str {
        match self {
            Self::Powershell => if cfg!(windows) { "powershell.exe" } else { "pwsh" },
            Self::Cmd => "cmd.exe",
            Self::Zsh => "/bin/zsh",
            Self::Bash => "/bin/bash",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    Default,
    AcceptEdits,
    Plan,
    Auto,
    DontAsk,
    BypassPermissions,
}

impl PermissionMode {
    pub fn is_unified_target(self) -> bool {
        matches!(
            self,
            Self::Default | Self::AcceptEdits | Self::Plan | Self::Auto
        )
    }
}

/// 会话级权限审批策略（Ask/Allow/Deny 三档）：
/// Ask 每次等待确认时出审批卡片由人工决定；Allow/Deny 由轮询直接代答。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApprovalPolicy {
    #[default]
    Ask,
    Allow,
    Deny,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub id: String,
    pub claude_session_id: String,
    #[serde(default)]
    pub kind: SessionKind,
    #[serde(default)]
    pub shell: Option<TerminalShell>,
    #[serde(default)]
    pub multi_agent_project_id: Option<String>,
    #[serde(default)]
    pub multi_agent_agent_id: Option<String>,
    #[serde(default)]
    pub agent_runtime_kind: Option<String>,
    #[serde(default)]
    pub retired_agent_runtime: bool,
    #[serde(default)]
    pub agent_avatar_id: Option<String>,
    /// 一次性任务的 PTY 是命令/日志宿主，不能接收聊天输入。
    #[serde(default)]
    pub runtime_command_host: bool,
    /// 归属的项目（projects.json）；None 表示未分组。
    #[serde(default)]
    pub project_id: Option<String>,
    /// 会话级独立 Git Worktree；None 表示直接在原目录运行。
    #[serde(default)]
    pub worktree: Option<SessionWorktree>,
    pub provider_id: String,
    pub provider_name: String,
    pub cwd: String,
    pub title: String,
    pub status: SessionStatus,
    pub pty_id: Option<u32>,
    pub yolo: bool,
    #[serde(default)]
    pub auto_mode: bool,
    #[serde(default)]
    pub permission_mode: Option<PermissionMode>,
    #[serde(default, skip_serializing)]
    pub permission_mode_revision: u64,
    #[serde(default)]
    pub approval_policy: ApprovalPolicy,
    /// 当前等待原因（Notification hook 的 message，如 "Claude needs your permission to use Bash"）。
    #[serde(default)]
    pub waiting_reason: Option<String>,
    /// 本轮等待的开始时间；同一轮等待只出一张卡片、只自动代答一次的依据。
    #[serde(default)]
    pub waiting_since: Option<u64>,
    pub title_locked: bool,
    pub exit_code: Option<u32>,
    pub last_error: Option<String>,
    /// 归档时间；存在时不会出现在普通会话列表中，但记录与项目归属仍保留。
    #[serde(default)]
    pub archived_at: Option<u64>,
    pub created_at: u64,
    pub updated_at: u64,
}

pub(crate) fn is_standalone_deepseek_harness(session: &SessionRecord) -> bool {
    session.kind == SessionKind::Terminal
        && session.agent_runtime_kind.as_deref() == Some("deepseek_harness")
        && session.provider_id == "local-runtime-deepseek_harness"
        && session.multi_agent_project_id.is_none()
        && session.multi_agent_agent_id.is_none()
}

pub(crate) fn is_standalone_claude_chat(session: &SessionRecord) -> bool {
    session.kind == SessionKind::Terminal
        && session.agent_runtime_kind.as_deref() == Some("claude_chat")
        && session.provider_id == "local-runtime-claude_chat"
        && session.multi_agent_project_id.is_none()
        && session.multi_agent_agent_id.is_none()
}

pub(crate) fn is_standalone_kimi_chat(session: &SessionRecord) -> bool {
    session.kind == SessionKind::Terminal
        && session.agent_runtime_kind.as_deref() == Some("kimi_chat")
        && session.provider_id == "local-runtime-kimi_chat"
        && session.multi_agent_project_id.is_none()
        && session.multi_agent_agent_id.is_none()
}

pub(crate) fn is_standalone_opencode_chat(session: &SessionRecord) -> bool {
    session.kind == SessionKind::Terminal
        && session.agent_runtime_kind.as_deref() == Some("opencode_chat")
        && session.provider_id == "local-runtime-opencode_chat"
        && session.multi_agent_project_id.is_none()
        && session.multi_agent_agent_id.is_none()
}

pub(crate) fn is_standalone_codex_chat(session: &SessionRecord) -> bool {
    session.kind == SessionKind::Terminal
        && session.agent_runtime_kind.as_deref() == Some("codex_chat")
        && session.provider_id == "local-runtime-codex_chat"
        && session.multi_agent_project_id.is_none()
        && session.multi_agent_agent_id.is_none()
}

pub(crate) fn is_standalone_pi_chat(session: &SessionRecord) -> bool {
    session.kind == SessionKind::Terminal
        && session.agent_runtime_kind.as_deref() == Some("pi_chat")
        && session.provider_id == "local-runtime-pi_chat"
        && session.multi_agent_project_id.is_none()
        && session.multi_agent_agent_id.is_none()
}

pub(crate) fn is_standalone_zcode(session: &SessionRecord) -> bool {
    session.kind == SessionKind::Terminal && (session.agent_runtime_kind.as_deref() == Some("zcode") || (session.agent_runtime_kind.as_deref() == Some("zcode_cli") && crate::session_switch::structured_zcode_cli(&session.id)))
        && matches!(session.provider_id.as_str(), "local-runtime-zcode" | "local-runtime-zcode_cli") && session.multi_agent_project_id.is_none() && session.multi_agent_agent_id.is_none()
}


pub struct NewSession {
    pub provider_id: String,
    pub provider_name: String,
    pub cwd: String,
    pub yolo: bool,
    pub auto_mode: bool,
    pub project_id: Option<String>,
}

pub struct NewTerminalSession {
    pub shell: TerminalShell,
    pub cwd: String,
    pub project_id: Option<String>,
}

pub struct NewRuntimeSession {
    pub runtime_kind: String,
    pub cwd: String,
    pub project_id: Option<String>,
}

pub struct NewAgentSession {
    pub avatar_id: Option<String>,
    pub project_id: String,
    pub agent_id: String,
    pub runtime_kind: String,
    pub agent_name: String,
    pub cwd: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionsFile {
    version: u32,
    sessions: Vec<SessionRecord>,
    /// 已成功生成标题的会话；与名称一起保存，重启后不重复调用模型。
    #[serde(default)]
    generated_titles: HashSet<String>,
    #[serde(default)]
    approval_generations: HashMap<String, u64>,
    #[serde(default)]
    approval_consumed: HashMap<String, u64>,
}

/// 只读加载持久化的会话记录，供 CLI 子命令（send/sessions）使用。
/// 不走 SessionManager::open_at：那个路径会做恢复性清理并写盘，
/// 在 app 运行中从外部进程调用会产生写竞争。
pub fn load_records(path: &std::path::Path) -> Result<Vec<SessionRecord>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)
        .with_context(|| format!("读取会话配置失败: {}", path.display()))?;
    let mut file: SessionsFile = serde_json::from_str(&raw)
        .with_context(|| format!("解析会话配置失败: {}", path.display()))?;
    for record in &mut file.sessions { retire_removed_runtime(record); }
    Ok(file.sessions)
}

fn retire_removed_runtime(record: &mut SessionRecord) -> bool {
    let Some(runtime) = record.agent_runtime_kind.as_deref() else { return false; };
    let kind: crate::multi_agent::AgentRuntimeKind = serde_json::from_value(serde_json::json!(runtime))
        .unwrap_or(crate::multi_agent::AgentRuntimeKind::Unsupported);
    if !kind.is_retired() { return false; }
    let changed = !record.retired_agent_runtime || record.pty_id.is_some() || record.status != SessionStatus::Stopped;
    record.retired_agent_runtime = true;
    record.pty_id = None;
    record.runtime_command_host = false;
    record.status = SessionStatus::Stopped;
    record.last_error = Some("该 CLI 已移除；历史记录保留，请使用其他 Runtime 新建会话".into());
    changed
}

impl Default for SessionsFile {
    fn default() -> Self {
        Self {
            version: SESSIONS_VERSION,
            sessions: Vec::new(),
            generated_titles: HashSet::new(),
            approval_generations: HashMap::new(),
            approval_consumed: HashMap::new(),
        }
    }
}

#[derive(Clone)]
pub struct SessionManager {
    lifecycle_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    // Readers affect one session; bulk binding changes take the write guard.
    // Neither this registry nor the session gate protects global session data
    // across I/O: unrelated data reads and individual sessions stay available.
    approval_registry: Arc<RwLock<()>>,
    approval_gates: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    starting_runtimes: Arc<Mutex<HashSet<String>>>,
    path: PathBuf,
    events_dir: PathBuf,
    event_ingest_lock: Arc<Mutex<()>>,
    data: Arc<Mutex<SessionsFile>>,
    ready_for_prompt: Arc<Mutex<HashSet<String>>>,
    runtime_prompt_buffers: Arc<Mutex<HashMap<String, RuntimePromptBuffer>>>,
    /// TUI dialogs/busy frames are independent of hook-level SessionStatus.
    /// Dispatch consumes readiness but must not erase a still-visible blocker.
    runtime_prompt_blockers: Arc<Mutex<HashMap<String, RuntimePromptBlocker>>>,
    runtime_bridge_tokens: Arc<Mutex<HashMap<String, (String, u64)>>>,
    runtime_bridge_errors: Arc<Mutex<HashMap<String, String>>>,
    active_background_sessions: Arc<Mutex<HashSet<String>>>,
    pending_first_prompts: Arc<Mutex<HashMap<String, String>>>,
    summarized_sessions: Arc<Mutex<HashSet<String>>>,
    summary_requests: Arc<Mutex<Vec<SummaryRequest>>>,
    agent_turns: Arc<Mutex<HashMap<String, AgentTurnCompletion>>>,
    active_agent_prompts: Arc<Mutex<HashMap<String, String>>>,
    transitions: Arc<Mutex<Vec<SessionTransition>>>,
    native_turn_outcomes: Arc<Mutex<HashMap<String, VecDeque<String>>>>,
}

/// Protect only spawn -> PTY binding, including early lifecycle events. Drop on errors too.
pub(crate) struct RuntimeStartGuard {
    starting: Arc<Mutex<HashSet<String>>>,
    id: String,
    _runtime_lease: Option<crate::runtime_preferences::RuntimeLease>,
}

impl Drop for RuntimeStartGuard {
    fn drop(&mut self) {
        self.starting.lock().unwrap().remove(&self.id);
    }
}

#[derive(Debug, Clone, Default)]
struct AgentTurnCompletion {
    revision: u64,
    summary: String,
    prompt: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RuntimePromptBlocker {
    Busy,
    Update,
    Dialog,
}

#[derive(Default)]
struct RuntimePromptBuffer {
    raw: String,
    zcode_screen: Option<vt100::Parser>,
    awaiting_composer_change: bool,
}

impl From<&str> for RuntimePromptBuffer {
    fn from(raw: &str) -> Self { Self { raw: raw.into(), ..Default::default() } }
}

impl RuntimePromptBuffer {
    fn zcode_screen(&mut self) -> &mut vt100::Parser {
        self.zcode_screen.get_or_insert_with(|| vt100::Parser::new(
            std::num::NonZeroU16::new(24).unwrap(), std::num::NonZeroU16::new(80).unwrap(), 0))
    }
}

/// 首轮对话结束后待生成的 AI 摘要标题请求。
#[derive(Debug, Clone)]
pub struct SummaryRequest {
    pub session_id: String,
    pub provider_id: String,
    pub prompt: String,
    pub runtime_kind: Option<String>,
    pub cwd: String,
    pub native_session_id: String,
    pub upstream_locator: Option<String>,
    pub model: Option<String>,
}

/// 传给摘要模型的首条消息上限，避免超长 prompt 浪费 token。
const SUMMARY_PROMPT_MAX_CHARS: usize = 1200;

fn cap_summary_prompt(prompt: &str) -> String {
    prompt.chars().take(SUMMARY_PROMPT_MAX_CHARS).collect()
}

impl SessionManager {
    pub fn new() -> Result<Self> {
        let config_dir = crate::providers::config_dir()?;
        Self::open_at(
            config_dir.join("sessions.json"),
            config_dir.join("session-events"),
        )
    }

    pub fn open_at(path: PathBuf, events_dir: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("创建会话配置目录失败: {}", parent.display()))?;
        }
        fs::create_dir_all(&events_dir)
            .with_context(|| format!("创建会话事件目录失败: {}", events_dir.display()))?;

        let mut data: SessionsFile = if path.exists() {
            let raw = fs::read_to_string(&path)
                .with_context(|| format!("读取会话配置失败: {}", path.display()))?;
            serde_json::from_str(&raw).context("解析 sessions.json 失败")?
        } else {
            SessionsFile::default()
        };
        let mut recovered = false;
        for session in &mut data.sessions {
            recovered |= retire_removed_runtime(session);
            if session.pty_id.take().is_some() {
                session.status = SessionStatus::Stopped;
                session.updated_at = now_millis();
                recovered = true;
            }
        }
        if recovered {
            save_sessions_at(&path, &data)
                .with_context(|| format!("恢复会话配置失败: {}", path.display()))?;
        }

        let generated_titles = data.generated_titles.clone();
        Ok(Self {
            lifecycle_locks: Arc::new(Mutex::new(HashMap::new())),
            approval_registry: Arc::new(RwLock::new(())),
            approval_gates: Arc::new(Mutex::new(HashMap::new())),
            starting_runtimes: Arc::new(Mutex::new(HashSet::new())),
            path,
            events_dir,
            event_ingest_lock: Arc::new(Mutex::new(())),
            data: Arc::new(Mutex::new(data)),
            ready_for_prompt: Arc::new(Mutex::new(HashSet::new())),
            runtime_prompt_buffers: Arc::new(Mutex::new(HashMap::new())),
            runtime_prompt_blockers: Arc::new(Mutex::new(HashMap::new())),
            runtime_bridge_tokens: Arc::new(Mutex::new(HashMap::new())),
            runtime_bridge_errors: Arc::new(Mutex::new(HashMap::new())),
            active_background_sessions: Arc::new(Mutex::new(HashSet::new())),
            pending_first_prompts: Arc::new(Mutex::new(HashMap::new())),
            summarized_sessions: Arc::new(Mutex::new(generated_titles)),
            summary_requests: Arc::new(Mutex::new(Vec::new())),
            agent_turns: Arc::new(Mutex::new(HashMap::new())),
            active_agent_prompts: Arc::new(Mutex::new(HashMap::new())),
            transitions: Arc::new(Mutex::new(Vec::new())),
            native_turn_outcomes: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn create(&self, input: NewSession) -> Result<SessionRecord> {
        crate::multi_agent::AgentRuntimeKind::ClaudeCode.ensure_available()?;
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        let record = SessionRecord {

            id: id.clone(),
            claude_session_id: id,
            kind: SessionKind::Claude,
            shell: None,
            multi_agent_project_id: None,
            multi_agent_agent_id: None,
            agent_runtime_kind: None,
            retired_agent_runtime: false,
            agent_avatar_id: None,
            runtime_command_host: false,
            project_id: input.project_id,
            worktree: None,
            provider_id: input.provider_id,
            provider_name: input.provider_name,
            cwd: input.cwd,
            title: "新会话".into(),
            status: SessionStatus::Pending,
            pty_id: None,
            yolo: input.yolo,
            auto_mode: input.auto_mode,
            permission_mode: None,
            permission_mode_revision: 0,
            approval_policy: ApprovalPolicy::default(),
            waiting_reason: None,
            waiting_since: None,
            title_locked: false,
            exit_code: None,
            last_error: None,
            archived_at: None,
            created_at: now,
            updated_at: now,
        };
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        data.sessions.push(record.clone());
        self.persist_candidate(&mut stored, data)?;
        Ok(record)
    }

    pub fn create_terminal(&self, input: NewTerminalSession) -> Result<SessionRecord> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        let shell_name = input.shell.display_name();
        let record = SessionRecord {

            id: id.clone(),
            claude_session_id: String::new(),
            kind: SessionKind::Terminal,
            shell: Some(input.shell),
            multi_agent_project_id: None,
            multi_agent_agent_id: None,
            agent_runtime_kind: None,
            retired_agent_runtime: false,
            agent_avatar_id: None,
            runtime_command_host: false,
            project_id: input.project_id,
            worktree: None,
            provider_id: "local-terminal".into(),
            provider_name: shell_name.into(),
            cwd: input.cwd,
            title: shell_name.into(),
            status: SessionStatus::Pending,
            pty_id: None,
            yolo: false,
            auto_mode: false,
            permission_mode: None,
            permission_mode_revision: 0,
            approval_policy: ApprovalPolicy::default(),
            waiting_reason: None,
            waiting_since: None,
            title_locked: false,
            exit_code: None,
            last_error: None,
            archived_at: None,
            created_at: now,
            updated_at: now,
        };
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        data.sessions.push(record.clone());
        self.persist_candidate(&mut stored, data)?;
        Ok(record)
    }

    /// 创建一个由 CrewVela 直接托管的交互式 Agent CLI 会话。
    ///
    /// 仍沿用 Terminal kind 以复用现有 PTY、网格和停止/删除能力，
    /// agent_runtime_kind 则负责区分普通 Shell 与各类交互式 Agent Runtime。
    pub fn create_runtime(&self, input: NewRuntimeSession) -> Result<SessionRecord> {
        self.create_runtime_with_id(input, uuid::Uuid::new_v4().to_string())
    }

    /// Reserved identity is persisted by the switch coordinator before this write.
    pub(crate) fn create_runtime_with_id(
        &self,
        input: NewRuntimeSession,
        id: String,
    ) -> Result<SessionRecord> {
        let kind: crate::multi_agent::AgentRuntimeKind =
            serde_json::from_value(serde_json::Value::String(input.runtime_kind.clone()))
                .context("Runtime 类型无效")?;
        kind.ensure_available()?;
        uuid::Uuid::parse_str(&id)?;
        if let Some(existing) = self.get(&id) {
            if existing.agent_runtime_kind.as_deref() != Some(&input.runtime_kind)
                || existing.cwd != input.cwd
                || existing.project_id != input.project_id
            {
                return Err(anyhow!("预留接手身份与已有会话不匹配"));
            }
            return Ok(existing);
        }
        let (runtime_kind, runtime_label) = match input.runtime_kind.as_str() {
            "claude_code" => ("claude_code", "Claude Code"),
            "codex" => ("codex", "Codex CLI"),
            "kimi_code" => ("kimi_code", "Kimi Code"),
            "minimax_code" => return Err(anyhow!("MiniMax CLI 已移除，不能新建会话")),
            "pi" => ("pi", "Pi Agent"),
            "oh_my_pi" => return Err(anyhow!("Oh My Pi 已移除，不能新建会话")),
            "opencode" => ("opencode", "OpenCode"),
            "claude_chat" => ("claude_chat", "Claude Code Chat"),
            "kimi_chat" => ("kimi_chat", "Kimi Code Chat"),
            "opencode_chat" => ("opencode_chat", "OpenCode Chat"),
            "pi_chat" => ("pi_chat", "Pi Chat"),
            "codex_chat" => ("codex_chat", "Codex Chat"),
            "deepseek_harness" => ("deepseek_harness", "DeepSeek Harness"),
            "zcode" => ("zcode", "ZCode Desktop"),
            "zcode_cli" => ("zcode_cli", "ZCode CLI"),
            value => return Err(anyhow!("暂不支持新建 Runtime 会话: {value}")),
        };
        let now = now_millis();
        let record = SessionRecord {

            id,
            claude_session_id: String::new(),
            kind: SessionKind::Terminal,
            shell: None,
            multi_agent_project_id: None,
            multi_agent_agent_id: None,
            agent_runtime_kind: Some(runtime_kind.to_string()),
            retired_agent_runtime: false,
            agent_avatar_id: None,
            runtime_command_host: false,
            project_id: input.project_id,
            worktree: None,
            provider_id: format!("local-runtime-{runtime_kind}"),
            provider_name: runtime_label.to_string(),
            cwd: input.cwd,
            title: runtime_label.to_string(),
            status: SessionStatus::Pending,
            pty_id: None,
            yolo: false,
            auto_mode: false,
            permission_mode: None,
            permission_mode_revision: 0,
            approval_policy: ApprovalPolicy::default(),
            waiting_reason: None,
            waiting_since: None,
            title_locked: false,
            exit_code: None,
            last_error: None,
            archived_at: None,
            created_at: now,
            updated_at: now,
        };
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        data.sessions.push(record.clone());
        self.persist_candidate(&mut stored, data)?;
        Ok(record)
    }

    pub fn create_agent(&self, input: NewAgentSession) -> Result<SessionRecord> {
        let kind: crate::multi_agent::AgentRuntimeKind =
            serde_json::from_value(serde_json::Value::String(input.runtime_kind.clone()))
                .context("Runtime 类型无效")?;
        kind.ensure_available()?;
        crate::multi_agent::validate_agent_avatar(input.avatar_id.as_deref())?;
        if matches!(input.runtime_kind.as_str(), "oh_my_pi" | "minimax_code") {
            return Err(anyhow!("该 CLI 已移除，不能创建 Agent 会话"));
        }
        if let Some(existing) = self
            .data
            .lock()
            .unwrap()
            .sessions
            .iter()
            .find(|session| {
                session.multi_agent_project_id.as_deref() == Some(input.project_id.as_str())
                    && session.multi_agent_agent_id.as_deref() == Some(input.agent_id.as_str())
                    && !session.retired_agent_runtime
                    && session.archived_at.is_none()
                    && session.agent_runtime_kind.as_deref() == Some(input.runtime_kind.as_str())
            })
            .cloned()
        {
            return Ok(existing);
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        let runtime_label = match input.runtime_kind.as_str() {
            "claude_code" => "Claude Code".to_string(),
            "codex" => "Codex".to_string(),
            "kimi_code" => "Kimi Code".to_string(),
            "pi" => "Pi".to_string(),
            "opencode" => "OpenCode".to_string(),
            "deepseek_harness" => "DeepSeek Harness".to_string(),
            "zcode" => "ZCode Desktop".to_string(),
            "zcode_cli" => "ZCode CLI".to_string(),
            _ => input.runtime_kind.clone(),
        };
        let record = SessionRecord {

            id,
            claude_session_id: String::new(),
            kind: SessionKind::Terminal,
            shell: Some(TerminalShell::agent_host_default()),
            multi_agent_project_id: Some(input.project_id),
            multi_agent_agent_id: Some(input.agent_id),
            agent_runtime_kind: Some(input.runtime_kind),
            retired_agent_runtime: false,
            agent_avatar_id: input.avatar_id,
            runtime_command_host: false,
            project_id: None,
            worktree: None,
            provider_id: "multi-agent-runtime".into(),
            provider_name: format!("{runtime_label} · {}", input.agent_name),
            cwd: input.cwd,
            title: format!("{runtime_label} · {}", input.agent_name),
            status: SessionStatus::Pending,
            pty_id: None,
            yolo: false,
            auto_mode: false,
            permission_mode: None,
            permission_mode_revision: 0,
            approval_policy: ApprovalPolicy::default(),
            waiting_reason: None,
            waiting_since: None,
            title_locked: true,
            exit_code: None,
            last_error: None,
            archived_at: None,
            created_at: now,
            updated_at: now,
        };
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        data.sessions.push(record.clone());
        self.persist_candidate(&mut stored, data)?;
        Ok(record)
    }

    pub fn list(&self) -> Vec<SessionRecord> {
        let mut sessions = self
            .data
            .lock()
            .unwrap()
            .sessions
            .iter()
            .filter(|session| session.archived_at.is_none())
            .cloned()
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.updated_at));
        sessions
    }

    pub fn list_archived(&self) -> Vec<SessionRecord> {
        let mut sessions = self
            .data
            .lock()
            .unwrap()
            .sessions
            .iter()
            .filter(|session| session.archived_at.is_some())
            .cloned()
            .collect::<Vec<_>>();
        sessions.sort_by_key(|session| std::cmp::Reverse(session.archived_at.unwrap_or_default()));
        sessions
    }

    pub fn archive(&self, id: &str) -> Result<SessionRecord> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        let now = now_millis();
        session.archived_at = Some(now);
        session.updated_at = now;
        let archived = session.clone();
        self.persist_candidate(&mut stored, data)?;
        Ok(archived)
    }

    pub fn restore(&self, id: &str) -> Result<SessionRecord> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        session.archived_at = None;
        session.updated_at = now_millis();
        let restored = session.clone();
        self.persist_candidate(&mut stored, data)?;
        Ok(restored)
    }

    pub fn archive_project_sessions(&self, project_id: &str) -> Result<usize> {
        let _approval_registry = self.approval_registry.write().unwrap();
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let now = now_millis();
        let mut archived_count = 0;
        for session in &mut data.sessions {
            if session.project_id.as_deref() == Some(project_id) && session.archived_at.is_none() {
                session.archived_at = Some(now);
                session.updated_at = now;
                archived_count += 1;
            }
        }
        if archived_count > 0 {
            self.persist_candidate(&mut stored, data)?;
        }
        Ok(archived_count)
    }

    pub fn hook_command(&self, app_session_id: &str) -> Result<String> {
        let executable = std::env::current_exe().context("无法确定 CrewVela 可执行文件")?;
        Ok(format!(
            "\"{}\" --session-hook \"{}\" \"{}\"",
            executable.display(),
            self.events_dir.display(),
            app_session_id
        ))
    }

    pub fn status_line_command(&self, app_session_id: &str) -> Result<String> {
        let executable = std::env::current_exe().context("无法确定 CrewVela 可执行文件")?;
        Ok(format!(
            "\"{}\" --session-statusline \"{}\" \"{}\"",
            executable.display(),
            self.events_dir.display(),
            app_session_id
        ))
    }

    pub fn agent_turn_hook_command(&self, app_session_id: &str) -> Result<Vec<String>> {
        let executable = std::env::current_exe().context("无法确定 CrewVela 可执行文件")?;
        Ok(vec![
            executable.display().to_string(),
            "--agent-turn-hook".to_string(),
            self.events_dir.display().to_string(),
            app_session_id.to_string(),
        ])
    }
    pub fn get(&self, id: &str) -> Option<SessionRecord> {
        self.data
            .lock()
            .unwrap()
            .sessions
            .iter()
            .find(|session| session.id == id)
            .cloned()
    }

    /// 旧原生会话作为只读历史保留；不可再成为成员的可执行收件箱。
    pub fn retire_agent_runtime_sessions(
        &self,
        project_id: &str,
        agent_id: &str,
    ) -> Result<Vec<SessionRecord>> {
        let _approval_registry = self.approval_registry.write().unwrap();
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let now = now_millis();
        let mut retired = Vec::new();
        for session in &mut data.sessions {
            if session.multi_agent_project_id.as_deref() == Some(project_id)
                && session.multi_agent_agent_id.as_deref() == Some(agent_id)
                && !session.retired_agent_runtime
            {
                retired.push(session.clone());
                session.retired_agent_runtime = true;
                session.archived_at = Some(now);
                session.status = SessionStatus::Stopped;
                session.pty_id = None;
                session.updated_at = now;
            }
        }
        self.persist_candidate(&mut stored, data)?;
        drop(stored);
        for session in &retired {
            self.ready_for_prompt.lock().unwrap().remove(&session.id);
            self.runtime_prompt_buffers.lock().unwrap().remove(&session.id);
            self.runtime_prompt_blockers.lock().unwrap().remove(&session.id);
        }
        Ok(retired)
    }

    pub fn sync_agent_profile(&self, profile: &crate::multi_agent::AgentProfile) -> Result<()> {
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let runtime_label = match profile.runtime_kind {
            crate::multi_agent::AgentRuntimeKind::ClaudeCode => "Claude Code",
            crate::multi_agent::AgentRuntimeKind::Codex => "Codex",
            crate::multi_agent::AgentRuntimeKind::KimiCode => "Kimi Code",
            crate::multi_agent::AgentRuntimeKind::MiniMaxCode | crate::multi_agent::AgentRuntimeKind::Unsupported => "已停用的 CLI",
            crate::multi_agent::AgentRuntimeKind::Pi => "Pi",
            crate::multi_agent::AgentRuntimeKind::OpenCode => "OpenCode",
            crate::multi_agent::AgentRuntimeKind::ClaudeChat => "Claude Code Chat",
            crate::multi_agent::AgentRuntimeKind::KimiChat => "Kimi Code Chat",
            crate::multi_agent::AgentRuntimeKind::OpenCodeChat => "OpenCode Chat",
            crate::multi_agent::AgentRuntimeKind::PiChat => "Pi Chat",
            crate::multi_agent::AgentRuntimeKind::CodexChat => "Codex Chat",
            crate::multi_agent::AgentRuntimeKind::DeepSeekHarness => "DeepSeek Harness",
            crate::multi_agent::AgentRuntimeKind::ZCode => "ZCode Desktop",
            crate::multi_agent::AgentRuntimeKind::ZCodeCli => "ZCode CLI",
            crate::multi_agent::AgentRuntimeKind::OhMyPi => "Oh My Pi（已移除）",
        };
        let label = format!("{runtime_label} · {}", profile.name);
        let mut changed = false;
        for session in &mut data.sessions {
            if session.multi_agent_agent_id.as_deref() == Some(profile.id.as_str())
                && session.multi_agent_project_id.as_deref() == Some(profile.project_id.as_str())
                && !session.retired_agent_runtime
                && session.agent_runtime_kind.as_deref() == Some(profile.runtime_kind.as_str())
            {
                session.agent_avatar_id = profile.avatar_id.clone();
                session.title = label.clone();
                session.provider_name = label.clone();
                session.updated_at = now_millis();
                changed = true;
            }
        }
        if changed {
            self.persist_candidate(&mut stored, data)?;
        }
        Ok(())
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<SessionRecord> {
        let title = title.trim();
        if title.is_empty() {
            return Err(anyhow!("会话标题不能为空"));
        }
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        session.title = title.to_string();
        session.title_locked = true;
        session.updated_at = now_millis();
        let updated = session.clone();
        self.persist_candidate(&mut stored, data)?;
        Ok(updated)
    }

    /// 把会话的工作目录切换到新建的独立 Worktree，并记录其元数据。
    pub fn assign_worktree(&self, id: &str, worktree: SessionWorktree) -> Result<SessionRecord> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        session.cwd = worktree.path.clone();
        session.worktree = Some(worktree);
        session.updated_at = now_millis();
        let updated = session.clone();
        self.persist_candidate(&mut stored, data)?;
        Ok(updated)
    }

    /// 设置会话的权限审批策略。只影响之后的等待；已出卡片的当前等待仍由人工决定。
    pub fn set_approval_policy(&self, id: &str, policy: ApprovalPolicy) -> Result<SessionRecord> {
        crate::session_switch::check_approval(id, policy)?;
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        session.approval_policy = policy;
        session.updated_at = now_millis();
        let updated = session.clone();
        self.persist_candidate(&mut stored, data)?;
        Ok(updated)
    }

    /// 解除某项目下所有会话的分组归属（删除项目时调用），返回清理的会话数。
    pub fn clear_project(&self, project_id: &str) -> Result<usize> {
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let now = now_millis();
        let mut cleared = 0usize;
        for session in &mut data.sessions {
            if session.project_id.as_deref() == Some(project_id) {
                session.project_id = None;
                session.updated_at = now;
                cleared += 1;
            }
        }
        if cleared > 0 {
            self.persist_candidate(&mut stored, data)?;
        }
        Ok(cleared)
    }

    pub fn drain_summary_requests(&self) -> Vec<SummaryRequest> {
        std::mem::take(&mut *self.summary_requests.lock().unwrap())
    }

    pub fn native_title_candidates(&self) -> Vec<SummaryRequest> {
        let generated = self.data.lock().unwrap().generated_titles.clone();
        self.list().into_iter().filter(|record| !generated.contains(&record.id) && !record.title_locked && !record.retired_agent_runtime && !record.runtime_command_host && record.archived_at.is_none()
            && matches!(record.agent_runtime_kind.as_deref(), Some("codex" | "kimi_code" | "opencode")))
            .filter_map(|record| {
                let locator = self.runtime_history_locator(&record).ok().flatten().or_else(|| (!record.claude_session_id.is_empty()).then(|| record.claude_session_id.clone()))?;
                Some(SummaryRequest { session_id: record.id, provider_id: record.provider_id, prompt: String::new(), runtime_kind: record.agent_runtime_kind,
                    cwd: record.cwd, native_session_id: record.claude_session_id, upstream_locator: Some(locator), model: None })
            }).collect()
    }

    /// 只在成功完成真实用户轮次后调用；不把摘要提示发送到原会话。
    pub fn queue_title_summary_for_model(&self, id: &str, prompt: &str, upstream_locator: Option<String>, model: Option<String>) {
        if prompt.trim().is_empty() || prompt.trim_start().starts_with('/') { return; }
        let data = self.data.lock().unwrap();
        let Some(record) = data.sessions.iter().find(|record| record.id == id) else { return; };
        if record.title_locked || record.retired_agent_runtime || record.runtime_command_host
            || data.generated_titles.contains(id) || record.archived_at.is_some()
            || matches!(record.agent_runtime_kind.as_deref(), Some("zcode" | "zcode_cli")) { return; }
        if !self.summarized_sessions.lock().unwrap().insert(id.to_string()) { return; }
        self.summary_requests.lock().unwrap().push(SummaryRequest {
            session_id: id.to_string(), provider_id: record.provider_id.clone(),
            prompt: cap_summary_prompt(prompt), runtime_kind: record.agent_runtime_kind.clone(),
            cwd: record.cwd.clone(), native_session_id: record.claude_session_id.clone(),
            upstream_locator,
            model,
        });
    }

    pub fn apply_title_request(&self, request: &SummaryRequest, title: &str) -> Result<Option<SessionRecord>> {
        self.apply_summary_title_inner(&request.session_id, title, Some(request))
    }

    /// 取走累计的状态跃迁（pty_poll_all 每轮调用，喂给自动化事件触发器）。
    pub fn drain_transitions(&self) -> Vec<SessionTransition> {
        std::mem::take(&mut *self.transitions.lock().unwrap())
    }

    pub fn restore_transitions(&self, mut transitions: Vec<SessionTransition>) {
        let mut queued = self.transitions.lock().unwrap();
        transitions.append(&mut *queued);
        *queued = transitions;
    }

    fn record_transition(&self, session_id: &str, from: SessionStatus, to: SessionStatus) {
        if from == to {
            return;
        }
        self.transitions.lock().unwrap().push(SessionTransition {
            session_id: session_id.to_string(),
            from,
            to,
        });
    }

    /// A turn outcome is independent of connection status: successful native chats
    /// remain Running so close/recovery checks still recognize the live connection.
    pub(crate) fn record_native_turn_outcome(&self, id: &str, turn_id: &str, outcome: SessionStatus) -> Result<()> {
        if turn_id.is_empty() || !matches!(outcome, SessionStatus::Completed | SessionStatus::Error | SessionStatus::Stopped) {
            return Err(anyhow!("原生轮次终态无效"));
        }
        let record = self.get(id).ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        crate::runtime_preferences::ensure_session_available(&record)?;
        let mut outcomes = self.native_turn_outcomes.lock().unwrap();
        let recent = outcomes.entry(id.to_string()).or_default();
        if recent.iter().any(|key| key == turn_id) { return Ok(()); }
        // Connections serialize and generation-check callbacks; retain recent identities
        // as additional protection against duplicate ACK and terminal-state messages.
        recent.push_back(turn_id.to_string());
        if recent.len() > 32 { recent.pop_front(); }
        self.record_transition(id, SessionStatus::Running, outcome);
        Ok(())
    }

    /// 应用 AI 摘要标题；会话不存在或已被手动重命名时跳过。
    pub fn apply_summary_title(&self, id: &str, title: &str) -> Result<Option<SessionRecord>> {
        self.apply_summary_title_inner(id, title, None)
    }

    fn apply_summary_title_inner(&self, id: &str, title: &str, request: Option<&SummaryRequest>) -> Result<Option<SessionRecord>> {
        let title = title.trim();
        if title.is_empty() || title.chars().any(char::is_control) {
            return Ok(None);
        }
        let mut data = self.data.lock().unwrap();
        if data.generated_titles.contains(id) && data.sessions.iter().any(|session| session.id == id && session.title == title) { return Ok(None); }
        let Some(session) = data.sessions.iter_mut().find(|session| session.id == id) else {
            return Ok(None);
        };
        if request.is_some_and(|request| session.agent_runtime_kind != request.runtime_kind
            || session.cwd != request.cwd || session.claude_session_id != request.native_session_id) { return Ok(None); }
        if session.title_locked || session.retired_agent_runtime || session.archived_at.is_some() {
            return Ok(None);
        }
        let previous = session.clone();
        let old_title = session.title.clone();
        session.title = title.to_string();
        session.updated_at = now_millis();
        let updated = session.clone();
        let already_generated = !data.generated_titles.insert(id.to_string());
        if let Err(error) = self.save(&data) {
            *data.sessions.iter_mut().find(|session| session.id == id).unwrap() = previous;
            if !already_generated { data.generated_titles.remove(id); }
            return Err(error);
        }
        eprintln!(
            "[crewvela] 摘要标题已写入 session={id} old=\"{old_title}\" new=\"{title}\" updated_at={}",
            updated.updated_at
        );
        Ok(Some(updated))
    }

    pub fn attach_runtime(&self, id: &str, pty_id: u32) -> Result<SessionRecord> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        self.ready_for_prompt.lock().unwrap().remove(id);
        self.runtime_prompt_buffers.lock().unwrap().remove(id);
        self.runtime_prompt_blockers.lock().unwrap().remove(id);
        self.active_background_sessions.lock().unwrap().remove(id);
        let mut data = self.data.lock().unwrap();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        session.pty_id = Some(pty_id);
        session.status = if session.kind == SessionKind::Terminal {
            SessionStatus::Running
        } else {
            SessionStatus::Pending
        };
        session.permission_mode = None;
        session.permission_mode_revision = 0;
        session.exit_code = None;
        session.last_error = None;
        session.updated_at = now_millis();
        let updated = session.clone();
        self.save(&data)?;
        Ok(updated)
    }

    /// 用户在等待权限或输入时提交了终端输入，Claude 会继续处理当前轮次。
    /// 仅响应带回车的真实提交；方向键等导航输入不会提前改变状态。
    pub fn mark_terminal_submission(&self, pty_id: u32) -> Result<Option<SessionRecord>> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let target_id = self.data.lock().unwrap().sessions.iter().find(|session| session.pty_id == Some(pty_id)).map(|session| session.id.clone());
        let Some(target_id) = target_id else { return Ok(None); };
        let approval_gate = self.approval_lock(&target_id);
        let _approval_gate = approval_gate.lock().unwrap();
        let mut data = self.data.lock().unwrap();
        let Some(session) = data
            .sessions
            .iter_mut()
            .find(|session| session.id == target_id && session.pty_id == Some(pty_id))
        else {
            return Ok(None);
        };
        // Pi slash commands/selector Enter are not agent input. Its API lifecycle owns readiness.
        if session.agent_runtime_kind.as_deref() == Some("pi") {
            return Ok(None);
        }
        self.mark_prompt_dispatched(&session.id);
        if session.status != SessionStatus::Waiting {
            return Ok(None);
        }

        let previous_status = session.status;
        session.status = SessionStatus::Running;
        session.waiting_reason = None;
        session.waiting_since = None;
        session.updated_at = now_millis();
        let updated = session.clone();
        self.save(&data)?;
        self.record_transition(&updated.id, previous_status, updated.status);
        Ok(Some(updated))
    }

    pub fn mark_agent_activity(
        &self,
        id: &str,
        status: SessionStatus,
        error: Option<String>,
    ) -> Result<SessionRecord> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        let mut data = self.data.lock().unwrap();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        if session.multi_agent_agent_id.is_none() {
            return Err(anyhow!("会话不是多 Agent 可视终端: {id}"));
        }
        let previous = session.clone();
        session.status = status;
        session.last_error = error;
        if status != SessionStatus::Waiting {
            session.waiting_reason = None;
            session.waiting_since = None;
        }
        session.updated_at = now_millis();
        let updated = session.clone();
        if let Err(error) = self.save(&data) {
            *data.sessions.iter_mut().find(|session| session.id == id).unwrap() = previous;
            return Err(error);
        }
        self.record_transition(id, previous.status, updated.status);
        Ok(updated)
    }

    pub(crate) fn mark_native_runtime_status(
        &self,
        id: &str,
        status: SessionStatus,
        waiting_reason: Option<String>,
        error: Option<String>,
    ) -> Result<SessionRecord> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        let mut data = self.data.lock().unwrap();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        let zcode_cli_projection = session.agent_runtime_kind.as_deref() == Some("zcode_cli") && session.provider_id == "local-runtime-zcode_cli" && session.multi_agent_project_id.is_none() && session.multi_agent_agent_id.is_none() && session.pty_id.is_none();
        if !is_standalone_deepseek_harness(session) && !is_standalone_zcode(session) && !zcode_cli_projection && !is_standalone_claude_chat(session) && !is_standalone_kimi_chat(session) && !is_standalone_codex_chat(session) && !is_standalone_pi_chat(session) && !is_standalone_opencode_chat(session) {
            return Err(anyhow!("会话不是独立原生 Agent 会话: {id}"));
        }
        let previous = session.clone();
        session.pty_id = None;
        session.status = status;
        session.waiting_reason = waiting_reason;
        session.waiting_since = if status == SessionStatus::Waiting {
            Some(now_millis())
        } else {
            None
        };
        session.last_error = error;
        session.updated_at = now_millis();
        let updated = session.clone();
        if let Err(error) = self.save(&data) {
            if let Some(session) = data.sessions.iter_mut().find(|session| session.id == id) {
                *session = previous;
            }
            return Err(error);
        }
        self.record_transition(id, previous.status, updated.status);
        Ok(updated)
    }

    pub fn mark_stopped(&self, id: &str) -> Result<SessionRecord> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        self.ready_for_prompt.lock().unwrap().remove(id);
        self.runtime_prompt_buffers.lock().unwrap().remove(id);
        self.runtime_prompt_blockers.lock().unwrap().remove(id);
        self.active_background_sessions.lock().unwrap().remove(id);
        self.active_agent_prompts.lock().unwrap().remove(id);
        let mut data = self.data.lock().unwrap();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        session.pty_id = None;
        session.status = SessionStatus::Stopped;
        session.waiting_reason = None;
        session.waiting_since = None;
        session.updated_at = now_millis();
        let updated = session.clone();
        self.save(&data)?;
        Ok(updated)
    }

    pub fn mark_all_stopped(&self) -> Result<()> {
        let _approval_registry = self.approval_registry.write().unwrap();
        self.ready_for_prompt.lock().unwrap().clear();
        self.runtime_prompt_buffers.lock().unwrap().clear();
        self.runtime_prompt_blockers.lock().unwrap().clear();
        self.active_background_sessions.lock().unwrap().clear();
        self.active_agent_prompts.lock().unwrap().clear();
        let mut data = self.data.lock().unwrap();
        let now = now_millis();
        let mut changed = false;
        for session in &mut data.sessions {
            if session.pty_id.take().is_some() {
                session.status = SessionStatus::Stopped;
                session.waiting_reason = None;
                session.waiting_since = None;
                session.updated_at = now;
                changed = true;
            }
        }
        if changed {
            self.save(&data)?;
        }
        Ok(())
    }

    pub fn mark_error(&self, id: &str, error: impl Into<String>) -> Result<SessionRecord> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        self.ready_for_prompt.lock().unwrap().remove(id);
        self.runtime_prompt_buffers.lock().unwrap().remove(id);
        self.runtime_prompt_blockers.lock().unwrap().remove(id);
        self.active_background_sessions.lock().unwrap().remove(id);
        self.active_agent_prompts.lock().unwrap().remove(id);
        let mut data = self.data.lock().unwrap();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        let previous_status = session.status;
        session.pty_id = None;
        session.status = SessionStatus::Error;
        session.last_error = Some(error.into());
        session.waiting_reason = None;
        session.waiting_since = None;
        session.updated_at = now_millis();
        let updated = session.clone();
        self.save(&data)?;
        self.record_transition(&updated.id, previous_status, updated.status);
        Ok(updated)
    }

    pub fn mark_exit(&self, id: &str, code: u32) -> Result<Option<SessionRecord>> {
        self.mark_exit_matching_pty(id, None, code)
    }

    pub(crate) fn mark_exit_if_pty(&self, id: &str, expected_pty_id: u32, code: u32) -> Result<Option<SessionRecord>> {
        self.mark_exit_matching_pty(id, Some(expected_pty_id), code)
    }

    fn mark_exit_matching_pty(&self, id: &str, expected_pty_id: Option<u32>, code: u32) -> Result<Option<SessionRecord>> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        if let Some(expected) = expected_pty_id {
            let data = self.data.lock().unwrap();
            if !data.sessions.iter().any(|session| session.id == id && session.pty_id == Some(expected)) {
                return Ok(None);
            }
        }
        self.ready_for_prompt.lock().unwrap().remove(id);
        self.runtime_prompt_buffers.lock().unwrap().remove(id);
        self.runtime_prompt_blockers.lock().unwrap().remove(id);
        self.active_background_sessions.lock().unwrap().remove(id);
        self.active_agent_prompts.lock().unwrap().remove(id);
        let mut data = self.data.lock().unwrap();
        let Some(session) = data.sessions.iter_mut().find(|session| session.id == id) else {
            return Ok(None);
        };
        let previous_status = session.status;
        let status = if code != 0 {
            SessionStatus::Error
        } else if session.status == SessionStatus::Completed {
            SessionStatus::Completed
        } else {
            SessionStatus::Stopped
        };
        if session.exit_code == Some(code) && session.status == status && session.pty_id.is_none() {
            return Ok(Some(session.clone()));
        }
        session.pty_id = None;
        session.exit_code = Some(code);
        session.status = status;
        session.waiting_reason = None;
        session.waiting_since = None;
        if code != 0 {
            session.last_error = Some(format!("会话已退出，代码 {code}"));
        }
        session.updated_at = now_millis();
        let updated = session.clone();
        self.save(&data)?;
        self.record_transition(&updated.id, previous_status, updated.status);
        Ok(Some(updated))
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let before = data.sessions.len();
        data.sessions.retain(|session| session.id != id);
        if data.sessions.len() == before {
            return Err(anyhow!("未找到会话: {id}"));
        }
        data.generated_titles.remove(id);
        data.approval_generations.remove(id);
        data.approval_consumed.remove(id);
        self.persist_candidate(&mut stored, data)?;
        drop(stored);
        self.ready_for_prompt.lock().unwrap().remove(id);
        self.runtime_prompt_buffers.lock().unwrap().remove(id);
        self.runtime_prompt_blockers.lock().unwrap().remove(id);
        self.active_background_sessions.lock().unwrap().remove(id);
        self.pending_first_prompts.lock().unwrap().remove(id);
        self.summarized_sessions.lock().unwrap().remove(id);
        self.summary_requests
            .lock()
            .unwrap()
            .retain(|request| request.session_id != id);
        self.agent_turns.lock().unwrap().remove(id);
        self.native_turn_outcomes.lock().unwrap().remove(id);
        self.active_agent_prompts.lock().unwrap().remove(id);
        Ok(())
    }

    /// Private per-launch Pi extension; events from previous processes cannot re-arm a new PTY.
    pub fn configure_pi_readiness(
        &self,
        command: &mut std::process::Command,
        id: &str,
    ) -> Result<String> {
        let directory = self
            .events_dir
            .parent()
            .unwrap_or(&self.events_dir)
            .join("runtime-extensions");
        fs::create_dir_all(&directory)?;
        let extension = directory.join("crewvela-pi-readiness.mjs");
        let source = include_str!("pi-readiness.mjs");
        if fs::read_to_string(&extension).ok().as_deref() != Some(source) {
            let temporary = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
            fs::write(&temporary, source)?;
            fs::rename(&temporary, &extension)?;
        }
        let token = uuid::Uuid::new_v4().to_string();
        self.runtime_bridge_tokens
            .lock()
            .unwrap()
            .insert(id.to_string(), (token.clone(), 0));
        self.ready_for_prompt.lock().unwrap().remove(id);
        let inbox = self.runtime_inbox_dir(id);
        fs::create_dir_all(&inbox)?;
        command.env("CREWVELA_PI_INBOX_DIR", inbox);
        command
            .arg("-e")
            .arg(extension)
            .env("CREWVELA_SESSION_ID", id)
            .env("CREWVELA_PI_EVENTS_DIR", &self.events_dir)
            .env("CREWVELA_PI_TOKEN", &token);
        Ok(token)
    }

    fn runtime_inbox_dir(&self, id: &str) -> PathBuf {
        let kind = self
            .get(id)
            .and_then(|session| session.agent_runtime_kind)
            .unwrap_or_default();
        self.events_dir
            .parent()
            .unwrap_or(&self.events_dir)
            .join(format!("{kind}-inbox"))
            .join(id)
    }

    pub fn runtime_message_accepted(&self, id: &str, message_id: &str) -> bool {
        let path = self
            .runtime_inbox_dir(id)
            .join(format!("{message_id}.accepted.json"));
        let accepted = fs::read(path)
            .ok()
            .and_then(|raw| serde_json::from_slice::<serde_json::Value>(&raw).ok())
            .is_some_and(|value| {
                value.get("sessionId").and_then(serde_json::Value::as_str) == Some(id)
                    && value.get("messageId").and_then(serde_json::Value::as_str)
                        == Some(message_id)
                    && value.get("status").and_then(serde_json::Value::as_str) == Some("accepted")
            });
        if accepted {
            let mut data = self.data.lock().unwrap();
            let owned = self.runtime_bridge_errors.lock().unwrap().remove(id);
            if let Some(session) = data.sessions.iter_mut().find(|record| record.id == id) {
                if owned.is_some() && session.last_error == owned {
                    session.last_error = None;
                }
            }
            if let Err(error) = self.save(&data) {
                eprintln!("[CrewVela] 保存Runtime收件确认状态失败: {error:#}");
            }
        }
        accepted
    }

    fn runtime_locator_path(&self, id: &str) -> PathBuf {
        self.events_dir
            .parent()
            .unwrap_or(&self.events_dir)
            .join("runtime-extensions")
            .join(format!("session-{id}.json"))
    }

    pub(crate) fn runtime_history_locator(&self, record: &SessionRecord) -> Result<Option<String>> {
        let path = self.runtime_locator_path(&record.id);
        if !path.exists() { return Ok(None); }
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        if value["runtime"].as_str() != record.agent_runtime_kind.as_deref() { return Err(anyhow!("原生历史运行时身份不匹配")); }
        Ok(value["locator"].as_str().map(str::to_owned))
    }
    pub fn configure_runtime_resume(
        &self,
        command: &mut std::process::Command,
        record: &SessionRecord,
    ) -> Result<()> {
        if (crate::session_switch::is_handoff_target(&record.id)? || crate::session_switch::is_handoff_source(&record.id)?) && matches!(record.agent_runtime_kind.as_deref(), Some("claude_code" | "codex" | "kimi_code" | "opencode")) {
            // Fresh handoff targets have no native history yet. Later launches
            // must recover the exact native identity instead of creating anew.
            if record.status == SessionStatus::Pending && record.created_at == record.updated_at { return Ok(()); }
            if record.agent_runtime_kind.as_deref() == Some("codex") && crate::session_switch::handoff_imported_thread(&record.id)?.is_some() { return Ok(()); }
            let locator = crate::session_switch_history::resume_locator(record, self)?;
            if locator.is_empty() || locator.len() > 256 || !locator.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')) {
                return Err(anyhow!("原生会话 ID 格式无效，拒绝恢复"));
            }
            match record.agent_runtime_kind.as_deref() {
                Some("claude_code") => { command.arg("--resume").arg(locator); },
                Some("codex") => { command.arg("resume").arg(locator); },
                _ => { command.arg("--session").arg(locator); },
            }
            return Ok(());
        }
        let Some(kind @ "pi") = record.agent_runtime_kind.as_deref() else {
            return Ok(());
        };
        let path = self.runtime_locator_path(&record.id);
        if !path.exists() {
            if record.status == SessionStatus::Pending && record.created_at == record.updated_at {
                return Ok(());
            }
            return Err(anyhow!("{kind}旧进程未记录原生会话位置，未新建替代会话；请在原Runtime手动恢复历史后重新打开受管会话，消息保留在原ID"));
        }
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
        let locator = value
            .get("locator")
            .and_then(serde_json::Value::as_str)
            .filter(|locator| !locator.is_empty())
            .ok_or_else(|| anyhow!("原Runtime会话位置无效，未新建替代会话"))?;
        if value.get("runtime").and_then(serde_json::Value::as_str) != Some(kind) {
            return Err(anyhow!("原Runtime会话类型不匹配，未新建替代会话"));
        }
        if !std::path::Path::new(locator).is_file() {
            return Err(anyhow!("Pi原会话文件不存在，未新建替代会话: {locator}"));
        }
        command.arg("--session").arg(locator);
        Ok(())
    }

    pub fn queue_runtime_delivery(&self, id: &str, message_id: &str, body: &str) -> Result<bool> {
        crate::runtime_preferences::ensure_session_available(
            &self.get(id).ok_or_else(|| anyhow!("会话不存在: {id}"))?,
        )?;
        let tokens = self.runtime_bridge_tokens.lock().unwrap();
        let Some((token, _)) = tokens.get(id) else {
            return Ok(false);
        };
        let directory = self.runtime_inbox_dir(id);
        // A claimed but unconfirmed delivery is uncertain; preserving it prevents duplicate replies.
        if directory
            .join(format!("{message_id}.processing.json"))
            .exists()
        {
            return Ok(false);
        }
        let target = directory.join(format!("{message_id}.request.json"));
        let temporary = directory.join(format!("{}.tmp", uuid::Uuid::new_v4()));
        fs::write(
            &temporary,
            serde_json::to_vec(&serde_json::json!({
                "sessionId": id, "messageId": message_id, "token": token, "body": body,
            }))?,
        )?;
        fs::rename(&temporary, target)?;
        Ok(true)
    }

    pub fn runtime_delivery_error(&self, id: &str, message_id: &str) -> Option<String> {
        if self.runtime_message_accepted(id, message_id) {
            return None;
        }
        let path = self
            .runtime_inbox_dir(id)
            .join(format!("{message_id}.unconfirmed.json"));
        let value: serde_json::Value = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
        if value.get("sessionId").and_then(serde_json::Value::as_str) != Some(id)
            || value.get("messageId").and_then(serde_json::Value::as_str) != Some(message_id)
        {
            return None;
        }
        value
            .get("error")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    }

    pub fn retire_runtime_task_delivery(&self, id: &str, message_id: &str) -> Result<()> {
        if !message_id.starts_with("task-") {
            return Err(anyhow!("仅可结束受管任务输入"));
        }
        let target = self
            .runtime_inbox_dir(id)
            .join(format!("{message_id}.retired.json"));
        let temporary = target.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        fs::write(
            &temporary,
            serde_json::to_vec(&serde_json::json!({"sessionId": id, "messageId": message_id}))?,
        )?;
        fs::rename(temporary, target)?;
        Ok(())
    }

    fn apply_pi_readiness(&self, event: &serde_json::Value) -> Result<Option<SessionRecord>> {
        let Some(id) = event
            .get(APP_SESSION_ID_FIELD)
            .and_then(serde_json::Value::as_str)
        else {
            return Ok(None);
        };
        if event.get("session_id").and_then(serde_json::Value::as_str) != Some(id)
            || !matches!(
                event
                    .get("runtime_kind")
                    .and_then(serde_json::Value::as_str),
                Some("pi")
            )
        {
            return Ok(None);
        }
        let Some(token) = event
            .get("runtime_token")
            .and_then(serde_json::Value::as_str)
        else {
            return Ok(None);
        };
        let Some(sequence) = event
            .get("runtime_sequence")
            .and_then(serde_json::Value::as_u64)
        else {
            return Ok(None);
        };
        let Some(ready) = event.get("ready").and_then(serde_json::Value::as_bool) else {
            return Ok(None);
        };
        let mut tokens = self.runtime_bridge_tokens.lock().unwrap();
        let Some((expected, last)) = tokens.get_mut(id) else {
            return Ok(None);
        };
        if expected != token || sequence <= *last {
            return Ok(None);
        }
        // Session data always precedes readiness when both guards are held.
        let mut data = self.data.lock().unwrap();
        let mut readiness = self.ready_for_prompt.lock().unwrap();
        let Some(session) = data.sessions.iter_mut().find(|record| {
            record.id == id
                && !record.retired_agent_runtime
                && record.agent_runtime_kind.as_deref()
                    == event
                        .get("runtime_kind")
                        .and_then(serde_json::Value::as_str)
                && !record.runtime_command_host
                && record.pty_id.is_some()
        }) else {
            return Ok(None);
        };
        let previous_status = session.status;
        if let Some(locator) = event
            .get("runtime_session_locator")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
        {
            fs::write(
                self.runtime_locator_path(id),
                serde_json::to_vec(&serde_json::json!({
                    "runtime": session.agent_runtime_kind, "locator": locator,
                }))?,
            )?;
        }
        if ready {
            readiness.insert(id.to_string());
            session.status = SessionStatus::Pending;
        } else {
            readiness.remove(id);
            session.status = SessionStatus::Running;
        }
        if let Some(error) = event
            .get("message_error")
            .and_then(serde_json::Value::as_str)
        {
            self.runtime_bridge_errors
                .lock()
                .unwrap()
                .insert(id.to_string(), error.to_string());
            session.last_error = Some(error.to_string());
            session.status = SessionStatus::Waiting;
        } else if ready {
            let owned = self.runtime_bridge_errors.lock().unwrap().remove(id);
            if owned.is_some() && session.last_error == owned {
                session.last_error = None;
            }
        }
        if session.agent_runtime_kind.as_deref() == Some("pi") {
            if let Some(completion) = event.get("turn_completion") {
                let reported = completion.get("prompt").and_then(serde_json::Value::as_str);
                let summary = completion
                    .get("summary")
                    .and_then(serde_json::Value::as_str);
                let mut active = self.active_agent_prompts.lock().unwrap();
                if let (Some(prompt), Some(summary)) = (reported, summary) {
                    if active.get(id).is_some_and(|expected| expected == prompt) {
                        active.remove(id);
                        if let Some(error) =
                            completion.get("error").and_then(serde_json::Value::as_str)
                        {
                            session.last_error = Some(error.to_string());
                        }
                        self.record_agent_turn_completion(
                            id,
                            summary.to_string(),
                            prompt.to_string(),
                        );
                    }
                }
            }
        }
        *last = sequence;
        session.updated_at = now_millis();
        let updated = session.clone();
        self.save(&data)?;
        self.record_transition(id, previous_status, updated.status);
        // Title queuing re-enters session data. Retire every outer guard first.
        drop(readiness);
        drop(tokens);
        drop(data);
        if event.get("runtime_kind").and_then(serde_json::Value::as_str) == Some("pi") {
            if let Some(completion) = event.get("turn_completion").filter(|completion| completion.get("error").is_none() || completion["error"].is_null()) {
                if let Some(prompt) = completion["prompt"].as_str() { self.queue_title_summary_for_model(id, prompt, None, completion["model"].as_str().map(str::to_string)); }
            }
        }
        Ok(Some(updated))
    }

    pub fn apply_hook_value(&self, event: &serde_json::Value) -> Result<Option<SessionRecord>> {
        self.apply_hook_value_with_codex_home(event, None)
    }

    fn apply_hook_value_with_codex_home(
        &self,
        event: &serde_json::Value,
        codex_home: Option<&std::path::Path>,
    ) -> Result<Option<SessionRecord>> {
        let normalized = crate::brand::normalize_hook(event)?;
        let event = &normalized;
        if event
            .get("hook_event_name")
            .and_then(serde_json::Value::as_str)
            == Some("RuntimePromptState")
        {
            return self.apply_pi_readiness(event);
        }
        let Some(reported_session_id) = event.get("session_id").and_then(|value| value.as_str())
        else {
            return Ok(None);
        };
        let claude_session_id = transcript_session_id(event).unwrap_or(reported_session_id);
        let Some(event_name) = event
            .get("hook_event_name")
            .and_then(|value| value.as_str())
        else {
            return Ok(None);
        };
        let app_session_id = event
            .get(APP_SESSION_ID_FIELD)
            .and_then(|value| value.as_str());


        let _approval_registry = self.approval_registry.read().unwrap();
        let target_id = {
            let data = self.data.lock().unwrap();
            data.sessions.iter().find(|session| match app_session_id {
                Some(id) => session.id == id,
                None => session.claude_session_id == claude_session_id || session.claude_session_id == reported_session_id,
            }).map(|session| session.id.clone())
        };
        let Some(target_id) = target_id else { return Ok(None); };
        let approval_gate = self.approval_lock(&target_id);
        let _approval_gate = approval_gate.lock().unwrap();
        // Match terminal submission/command-host lock order; hook ingestion must
        // never hold readiness while waiting for a submission's data guard.
        let mut data = self.data.lock().unwrap();
        let mut readiness = self.ready_for_prompt.lock().unwrap();
        let mut active_background_sessions = self.active_background_sessions.lock().unwrap();
        let session_index = if let Some(app_session_id) = app_session_id {
            data.sessions
                .iter()
                .position(|session| session.id == app_session_id)
        } else {
            data.sessions.iter().position(|session| {
                session.claude_session_id == claude_session_id
                    || session.claude_session_id == reported_session_id
            })
        };
        let Some(session_index) = session_index else {
            return Ok(None);
        };
        let waiting_generation = if event_name == "Notification"
            && matches!(event.get("notification_type").and_then(serde_json::Value::as_str), Some("permission_prompt" | "agent_needs_input"))
            && !data.sessions[session_index].retired_agent_runtime
            && data.sessions[session_index].claude_session_id == claude_session_id
        {
            let record = &data.sessions[session_index];
            let id = record.id.clone();
            let previous = data.approval_generations.get(&id).copied().unwrap_or(record.waiting_since.unwrap_or(record.updated_at));
            let next = now_millis().max(previous.checked_add(1).ok_or_else(|| anyhow!("审批代次已耗尽"))?);
            data.approval_generations.insert(id, next);
            Some(next)
        } else { None };
        let session = &mut data.sessions[session_index];
        if session.retired_agent_runtime {
            return Ok(None);
        }
        let previous_status = session.status;

        // Validate completion ownership before changing the native identity,
        // sidecar locator, permission state or completion revision.
        if event_name == "AgentTurnComplete" {
            let expected = self.active_agent_prompts.lock().unwrap().get(&session.id).cloned();
            if expected.is_some() && expected != event_prompt(event) {
                return Ok(None);
            }
        }
        let codex_completion = event_name == "AgentTurnComplete"
            && app_session_id == Some(session.id.as_str())
            && session.agent_runtime_kind.as_deref() == Some("codex");
        if codex_completion && session.claude_session_id != claude_session_id
            && !crate::session_switch_history::codex_interactive_identity(session, claude_session_id, codex_home, self.path.parent())?
        {
            return Ok(None);
        }
        if session.claude_session_id != claude_session_id {
            let can_rebind = matches!(event_name, "SessionStart" | "AgentTurnComplete")
                && app_session_id.is_some_and(|app_id| session.id == app_id)
                && (event_name == "SessionStart" || codex_completion || session.multi_agent_agent_id.is_some());
            if !can_rebind {
                return Ok(None);
            }
        }
        if codex_completion {
            let path = self.runtime_locator_path(&session.id);
            fs::create_dir_all(path.parent().context("缺少原生定位目录")?)?;
            fs::write(path, serde_json::to_vec(&serde_json::json!({"runtime":"codex", "locator":claude_session_id}))?)?;
        }
        if session.claude_session_id != claude_session_id {
            session.claude_session_id = claude_session_id.to_string();
        }

        let reported_mode = event
            .get("permission_mode")
            .cloned()
            .and_then(|value| serde_json::from_value::<PermissionMode>(value).ok());
        let mode_changed = reported_mode.is_some_and(|mode| {
            session.permission_mode_revision = session.permission_mode_revision.saturating_add(1);
            if session.permission_mode == Some(mode) {
                false
            } else {
                session.permission_mode = Some(mode);
                true
            }
        });

        let ready_for_prompt = match event_name {
            "SessionStart" => {
                active_background_sessions.remove(&session.id);
                session.status = SessionStatus::Pending;
                Some(true)
            }
            "UserPromptSubmit" => {
                session.status = SessionStatus::Running;
                if session.multi_agent_agent_id.is_some() {
                    if let Some(prompt) = event.get("prompt").and_then(serde_json::Value::as_str) {
                        let prompt = claude_pasted_transcript_text(prompt);
                        self.active_agent_prompts
                            .lock()
                            .unwrap()
                            .insert(session.id.clone(), prompt.to_string());
                    }
                }
                if !session.title_locked {
                    if let Some(prompt) = event.get("prompt").and_then(|value| value.as_str()) {
                        if session.title == "新会话" {
                            session.title = title_from_prompt(prompt);
                        }
                        // 记录首条消息，等首轮 Stop 后生成 AI 摘要标题。
                        let session_id = session.id.clone();
                        let summarized = self.summarized_sessions.lock().unwrap();
                        if !summarized.contains(&session_id) {
                            let mut pending = self.pending_first_prompts.lock().unwrap();
                            pending
                                .entry(session_id)
                                .or_insert_with(|| cap_summary_prompt(prompt));
                        }
                    }
                }
                Some(false)
            }
            "PostToolUse" | "PostToolUseFailure" => {
                session.status = SessionStatus::Running;
                Some(false)
            }
            "Stop" => {
                if has_background_tasks(event) {
                    active_background_sessions.insert(session.id.clone());
                    session.status = SessionStatus::Running;
                    Some(false)
                } else {
                    let reported_prompt = event_prompt(event);
                    let expected_prompt = self
                        .active_agent_prompts
                        .lock()
                        .unwrap()
                        .get(&session.id)
                        .cloned();
                    if expected_prompt.is_some()
                        && reported_prompt.is_some()
                        && expected_prompt != reported_prompt
                    {
                        return Ok(None);
                    }
                    active_background_sessions.remove(&session.id);
                    session.status = SessionStatus::Completed;
                    if session.multi_agent_agent_id.is_some() {
                        self.record_agent_turn_completion(
                            &session.id,
                            event_summary(event).unwrap_or_default(),
                            self.active_agent_prompts
                                .lock()
                                .unwrap()
                                .remove(&session.id)
                                .or_else(|| event_prompt(event))
                                .unwrap_or_default(),
                        );
                    }
                    // 首轮完成：把首条消息转成摘要请求，只触发一次。
                    if !session.title_locked {
                        let session_id = session.id.clone();
                        let provider_id = session.provider_id.clone();
                        let mut summarized = self.summarized_sessions.lock().unwrap();
                        if !summarized.contains(&session_id) {
                            let mut pending = self.pending_first_prompts.lock().unwrap();
                            if let Some(prompt) = pending.remove(&session_id) {
                                summarized.insert(session_id.clone());
                                self.summary_requests.lock().unwrap().push(SummaryRequest {
                                    session_id,
                                    provider_id,
                                    prompt,
                                    runtime_kind: session.agent_runtime_kind.clone(),
                                    cwd: session.cwd.clone(),
                                    native_session_id: session.claude_session_id.clone(),
                                    upstream_locator: Some(session.claude_session_id.clone()),
                                    model: None,
                                });
                            }
                        }
                    }
                    Some(true)
                }
            }
            "AgentTurnComplete" if session.multi_agent_agent_id.is_some() => {
                let reported_prompt = event_prompt(event);
                active_background_sessions.remove(&session.id);
                session.status = SessionStatus::Completed;
                self.record_agent_turn_completion(
                    &session.id,
                    event_summary(event).unwrap_or_default(),
                    self.active_agent_prompts
                        .lock()
                        .unwrap()
                        .remove(&session.id)
                        .or(reported_prompt)
                        .unwrap_or_default(),
                );
                Some(true)
            }
            "Notification" => match event
                .get("notification_type")
                .and_then(|value| value.as_str())
            {
                Some("permission_prompt") | Some("agent_needs_input") => {
                    session.status = SessionStatus::Waiting;
                    session.waiting_since = waiting_generation;
                    session.waiting_reason = event
                        .get("message")
                        .and_then(|value| value.as_str())
                        .map(str::to_string);
                    Some(false)
                }
                Some("idle_prompt") | Some("agent_completed") => {
                    if session.status == SessionStatus::Running
                        || active_background_sessions.contains(&session.id)
                    {
                        Some(false)
                    } else {
                        session.status = SessionStatus::Completed;
                        Some(true)
                    }
                }
                _ => None,
            },
            "SessionEnd" => {
                active_background_sessions.remove(&session.id);
                Some(false)
            }
            _ => None,
        };
        // 离开等待态时同步清理等待元数据；等待态的刷新（如重复通知）保留新一轮的时间戳。
        if session.status != SessionStatus::Waiting {
            session.waiting_reason = None;
            session.waiting_since = None;
        }
        if let Some(ready_for_prompt) = ready_for_prompt {
            if ready_for_prompt {
                if codex_completion {
                    // A validated completion ends the previous busy turn, but
                    // cannot dismiss a permission, login or update dialog.
                    let mut blockers = self.runtime_prompt_blockers.lock().unwrap();
                    if blockers.get(&session.id) == Some(&RuntimePromptBlocker::Busy) {
                        blockers.remove(&session.id);
                    }
                }
                readiness.insert(session.id.clone());
            } else {
                readiness.remove(&session.id);
            }
        } else if !mode_changed {
            return Ok(None);
        }
        session.updated_at = now_millis();
        let updated = session.clone();
        self.save(&data)?;
        self.record_transition(&updated.id, previous_status, updated.status);
        Ok(Some(updated))
    }

    pub fn ingest_events(&self) -> Result<usize> {
        let _ingest_guard = self.event_ingest_lock.lock().unwrap();
        let event_paths = fs::read_dir(&self.events_dir)
            .with_context(|| format!("读取会话事件目录失败: {}", self.events_dir.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>();

        let mut events = Vec::with_capacity(event_paths.len());
        for path in event_paths {
            let raw = match fs::read_to_string(&path) {
                Ok(raw) => raw,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("读取会话事件失败: {}", path.display()));
                }
            };
            let event: serde_json::Value = serde_json::from_str(&raw)
                .with_context(|| format!("解析会话事件失败: {}", path.display()))?;
            let event = crate::brand::normalize_hook(&event)?;
            let queued_at = event
                .get("_crewvela_queued_at")
                .and_then(|value| value.as_u64())
                .unwrap_or_default();
            events.push((queued_at, path, event));
        }
        events.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        let mut ingested = 0;
        for (_, path, event) in events {
            let event_name = event
                .get("hook_event_name")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let event_session_id = event
                .get("session_id")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let event_app_id = event
                .get(APP_SESSION_ID_FIELD)
                .and_then(|value| value.as_str())
                .unwrap_or("");
            // Another desktop process can share this legacy event directory.
            // Unknown identities belong to its owner, not to this consumer.
            if (!event_app_id.is_empty() || !event_session_id.is_empty())
                && !self.data.lock().unwrap().sessions.iter().any(|session| {
                    if !event_app_id.is_empty() { session.id == event_app_id }
                    else { session.claude_session_id == event_session_id }
                })
            {
                continue;
            }
            // A process may emit session_start before open() returns its PTY. Keep
            // its event on disk until attach_runtime has finished clearing old state.
            if self
                .starting_runtimes
                .lock()
                .unwrap()
                .contains(event_app_id)
            {
                continue;
            }
            let is_stop = event_name == "Stop";
            let outcome = self.apply_hook_value(&event);
            if let Err(error) = &outcome {
                eprintln!(
                    "[crewvela] hook 处理失败 event={event_name} claude_sid={event_session_id} app_sid={event_app_id} err={error:#}"
                );
                remove_ingested_event(&path)?;
                continue;
            }
            if is_stop {
                let queued = self.summary_requests.lock().unwrap().len();
                eprintln!(
                    "[crewvela] Stop 事件已处理 event={event_name} claude_sid={event_session_id} app_sid={event_app_id} 待摘要队列={queued}"
                );
            }
            remove_ingested_event(&path)?;
            ingested += 1;
        }
        Ok(ingested)
    }

    pub fn is_ready_for_prompt(&self, id: &str) -> bool {
        // When holding both prompt guards, readiness always precedes blockers.
        // A completion hook alone cannot dismiss a visible update/login dialog.
        let ready = self.ready_for_prompt.lock().unwrap();
        ready.contains(id) && !self.runtime_prompt_blockers.lock().unwrap().contains_key(id)
    }

    pub fn runtime_prompt_is_blocked(&self, id: &str) -> bool {
        self.runtime_prompt_blockers.lock().unwrap().contains_key(id)
    }

    fn approval_lock(&self, id: &str) -> Arc<Mutex<()>> {
        self.approval_gates.lock().unwrap().entry(id.to_string()).or_insert_with(|| Arc::new(Mutex::new(()))).clone()
    }

    pub(crate) fn respond_waiting_approval(
        &self, id: &str, expected_pty_id: u32, expected_waiting_since: u64,
        allow: bool, writer: impl FnOnce(u32) -> Result<()>,
    ) -> Result<SessionRecord> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        if self.starting_runtimes.lock().unwrap().contains(id) { return Err(anyhow!("会话正在启动，审批状态已失效")); }
        {
            let data = self.data.lock().unwrap();
            let session = data.sessions.iter().find(|session| session.id == id).ok_or_else(|| anyhow!("审批会话已不存在"))?;
            let claude = session.kind == SessionKind::Claude || session.agent_runtime_kind.as_deref() == Some("claude_code");
            if !claude || session.runtime_command_host || session.archived_at.is_some() || session.retired_agent_runtime
                || session.status != SessionStatus::Waiting || session.pty_id != Some(expected_pty_id)
                || session.waiting_since != Some(expected_waiting_since)
                || data.approval_consumed.get(id).is_some_and(|consumed| *consumed >= expected_waiting_since)
            { return Err(anyhow!("审批目标或等待代次已变化，请刷新当前会话")); }
        }
        // No global data guard crosses this potentially blocked pipe write.
        let write_result = writer(expected_pty_id);
        let mut data = self.data.lock().unwrap();
        data.approval_consumed.insert(id.to_string(), expected_waiting_since);
        let session = data.sessions.iter_mut().find(|session| session.id == id).ok_or_else(|| anyhow!("审批输入后会话已不存在，禁止重放"))?;
        let previous_status = session.status;
        session.waiting_since = None;
        session.waiting_reason = None;
        session.updated_at = now_millis();
        match &write_result {
            Ok(()) => { session.status = SessionStatus::Running; session.last_error = None; },
            Err(error) => { session.status = SessionStatus::Waiting; session.last_error = Some(format!("审批输入可能已部分发出，当前代次已消费，请核对终端后操作：{error:#}")); },
        }
        let updated = session.clone();
        if allow && write_result.is_ok() { self.mark_prompt_dispatched(id); }
        let saved = self.save(&data);
        self.record_transition(id, previous_status, updated.status);
        if let Err(error) = saved { return Err(anyhow!("审批输入已尝试发出且代次已消费，但确认落盘失败；禁止重放：{error:#}")); }
        write_result.context("审批输入确认失败，代次已消费，禁止重复提交")?;
        Ok(updated)
    }

    pub(crate) fn lifecycle_lock(&self, id: &str) -> Arc<Mutex<()>> {
        self.lifecycle_locks
            .lock()
            .unwrap()
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    pub(crate) fn begin_runtime_start(&self, id: &str) -> Result<RuntimeStartGuard> {
        let record = self.get(id).ok_or_else(|| anyhow!("会话不存在: {id}"))?;
        let runtime_lease = crate::runtime_preferences::session_runtime(&record)?
            .map(crate::runtime_preferences::acquire)
            .transpose()?;
        // Serialize with event application before exposing a fresh launch window.
        let _ingest = self.event_ingest_lock.lock().unwrap();
        let mut starting = self.starting_runtimes.lock().unwrap();
        if !starting.insert(id.to_string()) {
            return Err(anyhow!("会话正在启动: {id}"));
        }
        let guard = RuntimeStartGuard {
            starting: self.starting_runtimes.clone(),
            id: id.to_string(),
            _runtime_lease: runtime_lease,
        };
        drop(starting);
        self.set_runtime_command_host(id, false)?;
        Ok(guard)
    }

    /// Startup has no PTY yet; it is not an exited legacy command host.
    pub(crate) fn migrate_inactive_runtime_command_host(
        &self,
        id: &str,
        is_running: impl FnOnce(u32) -> bool,
    ) -> Result<bool> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        let starting = self.starting_runtimes.lock().unwrap();
        if starting.contains(id) {
            return Ok(false);
        }
        drop(starting);
        let Some(record) = self.get(id) else {
            return Ok(false);
        };
        if record.runtime_command_host || record.pty_id.is_some_and(is_running) {
            return Ok(false);
        }
        self.set_runtime_command_host_inner(id, true)?;
        Ok(true)
    }

    /// 配置记录只是启动证据，不等同于子进程已读到环境变量或桥已确认。
    pub(crate) fn communication_bridge_evidence(&self, id: &str) -> serde_json::Value {
        serde_json::json!({
            "configured": self.runtime_bridge_tokens.lock().unwrap().contains_key(id),
            "error": self.runtime_bridge_errors.lock().unwrap().get(id).cloned(),
        })
    }

    pub fn mark_runtime_prompt_ready(&self, id: &str) -> bool {
        let data = self.data.lock().unwrap();
        let eligible = data.sessions.iter().any(|session| {
            session.id == id
                && session.kind == SessionKind::Terminal
                && session.agent_runtime_kind.is_some()
                && !session.runtime_command_host
                && session.pty_id.is_some()
        });
        if !eligible { return false; }
        let mut readiness = self.ready_for_prompt.lock().unwrap();
        // Only an explicitly recognized fresh runtime prompt dismisses a TUI
        // blocker. mark_prompt_dispatched intentionally leaves it intact.
        self.runtime_prompt_blockers.lock().unwrap().remove(id);
        readiness.insert(id.to_string())
    }

    #[cfg(test)]
    pub fn observe_runtime_prompt_output(
        &self,
        id: &str,
        data: &str,
        is_ready: impl FnOnce(&str) -> bool,
    ) -> bool {
        self.observe_runtime_prompt_output_with_blocker(id, data, is_ready, |_| false)
    }

    pub fn observe_runtime_prompt_output_with_blocker(
        &self,
        id: &str,
        data: &str,
        is_ready: impl FnOnce(&str) -> bool,
        is_blocked: impl FnOnce(&str) -> bool,
    ) -> bool {
        if data.is_empty() {
            return false;
        }
        if self.get(id).is_some_and(|session| session.agent_runtime_kind.as_deref() == Some("zcode_cli")) {
            // OpenTUI repaints cells out of reading order. Inspect the visible
            // screen, not concatenated historical help/permission text.
            let mut buffers = self.runtime_prompt_buffers.lock().unwrap();
            let state = buffers.entry(id.to_string()).or_default();
            state.zcode_screen().process(data.as_bytes());
            let screen = state.zcode_screen().screen().contents();
            let blocked = is_blocked(&screen);
            let ready = !blocked && is_ready(&screen);
            if !ready { state.awaiting_composer_change = false; }
            let ready = ready && !state.awaiting_composer_change;
            drop(buffers);
            if ready { return self.mark_runtime_prompt_ready(id); }
            self.ready_for_prompt.lock().unwrap().remove(id);
            if blocked {
                self.runtime_prompt_blockers.lock().unwrap().insert(id.to_string(),
                    if crate::multi_agent::interactive_runtime_prompt_busy(&screen) { RuntimePromptBlocker::Busy } else { RuntimePromptBlocker::Dialog });
            }
            return false;
        }
        // Completion is authoritative for Codex's finished turn. Subsequent
        // cursor/footer fragments need not redraw its entire input composer.
        // Explicit blockers and unknown replacement screens still revoke it.
        let session = self.get(id);
        let is_codex = session.as_ref().is_some_and(|session| session.agent_runtime_kind.as_deref() == Some("codex"));
        let completed_codex = is_codex && session.is_some_and(|session| session.status == SessionStatus::Completed);
        let mut buffers = self.runtime_prompt_buffers.lock().unwrap();
        let buffer = &mut buffers.entry(id.to_string()).or_default().raw;
        buffer.push_str(data);
        if buffer.len() > 32_768 {
            let mut start = buffer.len() - 32_768;
            while !buffer.is_char_boundary(start) {
                start += 1;
            }
            buffer.drain(..start);
        }
        let blocked = is_blocked(buffer);
        let busy = blocked && crate::multi_agent::interactive_runtime_prompt_busy(buffer);
        if is_codex {
            if let Some((ready, end)) = crate::multi_agent::codex_terminal_state(buffer) {
                let mut readiness = self.ready_for_prompt.lock().unwrap();
                let mut blockers = self.runtime_prompt_blockers.lock().unwrap();
                if !ready {
                    readiness.remove(id);
                    blockers.entry(id.to_string()).or_insert(RuntimePromptBlocker::Busy);
                    buffer.clear();
                    return false;
                }
                // Native idle follows startup/update initialization and turn
                // hooks, even when the TUI does not repaint its composer.
                // It cannot dismiss login/permission dialogs or a later blocker.
                if (!blocked || busy)
                    && !crate::multi_agent::interactive_runtime_prompt_blocked(&buffer[end..])
                    && blockers.get(id) != Some(&RuntimePromptBlocker::Dialog) {
                    blockers.remove(id);
                    readiness.insert(id.to_string());
                    // Retain this recognized native frame until dispatch,
                    // busy output or a screen replacement invalidates it.
                    return true;
                }
            }
        }
        if blocked {
            let blocker = if is_codex && crate::multi_agent::codex_native_update_prompt(buffer) {
                RuntimePromptBlocker::Update
            } else if busy {
                RuntimePromptBlocker::Busy
            } else {
                RuntimePromptBlocker::Dialog
            };
            buffer.clear();
            let mut readiness = self.ready_for_prompt.lock().unwrap();
            self.runtime_prompt_blockers.lock().unwrap().entry(id.to_string())
                .and_modify(|previous| {
                    if blocker == RuntimePromptBlocker::Dialog
                        || (*previous == RuntimePromptBlocker::Busy && blocker == RuntimePromptBlocker::Update)
                    { *previous = blocker; }
                })
                .or_insert(blocker);
            readiness.remove(id);
            return false;
        }
        if !is_ready(buffer) {
            // Cursor-home is also emitted by partial footer redraws after a
            // completed Codex tool turn; it does not erase the idle screen.
            let replaces_screen = ["\x1b[J", "\x1b[0J", "\x1b[2J"].iter().any(|marker| data.contains(marker));
            if !completed_codex || replaces_screen {
                self.ready_for_prompt.lock().unwrap().remove(id);
            }
            return false;
        }
        // Keep the bounded, recognized frame until dispatch or an explicit
        // blocker invalidates it. TUIs often emit cursor/color resets, blank
        // lines, and footer fragments after drawing their idle composer; those
        // chunks alone cannot re-establish the composer if we discard it here.
        // runtime_prompt_frame still excludes the old frame after a new screen
        // clear, so retaining it does not make an unknown screen input-ready.
        drop(buffers);
        self.mark_runtime_prompt_ready(id)
    }

    pub fn mark_prompt_dispatched(&self, id: &str) {
        self.ready_for_prompt.lock().unwrap().remove(id);
        let mut buffers = self.runtime_prompt_buffers.lock().unwrap();
        if let Some(state) = buffers.get_mut(id).filter(|state| state.zcode_screen.is_some()) {
            // Keep the terminal's cell state for incremental redraws, but an
            // unchanged idle screen cannot authorize a second paste.
            state.awaiting_composer_change = crate::multi_agent::interactive_runtime_prompt_ready(
                crate::multi_agent::AgentRuntimeKind::ZCodeCli, &state.zcode_screen().screen().contents());
        } else {
            buffers.remove(id);
        }
    }

    pub(crate) fn resize_runtime_prompt(&self, id: &str, rows: u16, cols: u16) {
        if !self.get(id).is_some_and(|session| session.agent_runtime_kind.as_deref() == Some("zcode_cli")) { return; }
        let (Some(rows), Some(cols)) = (std::num::NonZeroU16::new(rows.min(512)), std::num::NonZeroU16::new(cols.min(512))) else { return; };
        self.runtime_prompt_buffers.lock().unwrap().entry(id.to_string()).or_default().zcode_screen().set_size(rows, cols);
    }

    pub fn set_runtime_command_host(&self, id: &str, enabled: bool) -> Result<()> {
        let _approval_registry = self.approval_registry.read().unwrap();
        let approval_gate = self.approval_lock(id);
        let _approval_gate = approval_gate.lock().unwrap();
        self.set_runtime_command_host_inner(id, enabled)
    }

    fn set_runtime_command_host_inner(&self, id: &str, enabled: bool) -> Result<()> {
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let session = data
            .sessions
            .iter_mut()
            .find(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        let clear_prompt = enabled || session.runtime_command_host != enabled;
        session.runtime_command_host = enabled;
        self.persist_candidate(&mut stored, data)?;
        drop(stored);
        if clear_prompt {
            self.mark_prompt_dispatched(id);
        }
        Ok(())
    }

    pub fn register_agent_prompt(&self, id: &str, prompt: &str) {
        self.ready_for_prompt.lock().unwrap().remove(id);
        self.active_agent_prompts
            .lock()
            .unwrap()
            .insert(id.to_string(), prompt.to_string());
    }

    pub fn clear_agent_prompt(&self, id: &str) {
        self.active_agent_prompts.lock().unwrap().remove(id);
    }

    pub fn complete_agent_turn_fallback(
        &self,
        id: &str,
        prompt: &str,
        summary: &str,
    ) -> Result<SessionRecord> {
        self.record_agent_turn_completion(id, summary.to_string(), prompt.to_string());
        self.active_agent_prompts.lock().unwrap().remove(id);
        self.ready_for_prompt.lock().unwrap().insert(id.to_string());
        self.mark_agent_activity(id, SessionStatus::Completed, None)
    }

    pub fn agent_turn_revision(&self, id: &str) -> u64 {
        self.agent_turns
            .lock()
            .unwrap()
            .get(id)
            .map_or(0, |turn| turn.revision)
    }

    pub fn agent_turn_summary_after(
        &self,
        id: &str,
        revision: u64,
        expected_prompt: &str,
    ) -> Option<String> {
        self.agent_turns
            .lock()
            .unwrap()
            .get(id)
            .filter(|turn| turn.revision > revision && turn.prompt == expected_prompt)
            .map(|turn| turn.summary.clone())
    }

    fn record_agent_turn_completion(&self, id: &str, summary: String, prompt: String) {
        let mut turns = self.agent_turns.lock().unwrap();
        let turn = turns.entry(id.to_string()).or_default();
        turn.revision = turn.revision.saturating_add(1);
        turn.summary = summary;
        turn.prompt = prompt;
    }

    pub(crate) fn set_handoff_plan(&self, id: &str) -> Result<()> {
        let mut stored = self.data.lock().unwrap();
        let mut data = stored.clone();
        let session = data.sessions.iter_mut().find(|session| session.id == id).context("接手会话不存在")?;
        session.permission_mode = Some(PermissionMode::Plan);
        self.persist_candidate(&mut stored, data)
    }
    pub fn predict_permission_mode_if_revision(
        &self,
        id: &str,
        mode: PermissionMode,
        expected_revision: u64,
    ) -> Result<bool> {
        let mut data = self.data.lock().unwrap();
        let index = data
            .sessions
            .iter()
            .position(|session| session.id == id)
            .ok_or_else(|| anyhow!("未找到会话: {id}"))?;
        if data.sessions[index].permission_mode_revision != expected_revision {
            return Ok(false);
        }
        if data.sessions[index].permission_mode == Some(mode) {
            return Ok(true);
        }

        data.sessions[index].permission_mode = Some(mode);
        data.sessions[index].updated_at = now_millis();
        Ok(true)
    }

    // Administrative edits publish only after durable storage accepts the full candidate.
    // Runtime facts and consumed approvals retain their separate, non-replayable semantics.
    fn persist_candidate(&self, current: &mut SessionsFile, candidate: SessionsFile) -> Result<()> {
        self.save(&candidate)?;
        *current = candidate;
        Ok(())
    }

    fn save(&self, data: &SessionsFile) -> Result<()> {
        save_sessions_at(&self.path, data)
            .with_context(|| format!("写入会话配置失败: {}", self.path.display()))
    }
}

fn save_sessions_at(path: &std::path::Path, data: &SessionsFile) -> Result<()> {
    save_sessions_with(path, data, |source, destination| fs::rename(source, destination))
}

fn save_sessions_with(
    path: &std::path::Path,
    data: &SessionsFile,
    publish: impl FnOnce(&std::path::Path, &std::path::Path) -> std::io::Result<()>,
) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(data)?;
    let previous = match fs::metadata(path) {
        Ok(metadata) => Some(metadata.permissions()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    if previous.as_ref().is_some_and(|permissions| permissions.readonly()) {
        return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "会话配置为只读文件").into());
    }
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
        let mut file = options.open(&temporary).context("创建会话临时文件失败")?;
        if let Some(permissions) = previous { file.set_permissions(permissions).context("保留会话文件权限失败")?; }
        file.write_all(&bytes).context("写入会话临时文件失败")?;
        file.sync_all().context("同步会话临时文件失败")?;
        drop(file);
        publish(&temporary, path).context("原子替换会话文件失败")?;
        Ok(())
    })();
    if temporary.exists() {
        fs::remove_file(&temporary).with_context(|| format!("清理会话配置临时文件失败：{}", temporary.display()))?;
    }
    result
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {

    #[test]
    fn terminal_shell_serialization_keeps_legacy_and_unix_sessions() {
        for (shell, name) in [(TerminalShell::Powershell, "powershell"), (TerminalShell::Cmd, "cmd"), (TerminalShell::Zsh, "zsh"), (TerminalShell::Bash, "bash")] {
            let json = serde_json::to_string(&shell).unwrap();
            assert_eq!(json, format!("\"{name}\""));
            assert_eq!(serde_json::from_str::<TerminalShell>(&json).unwrap(), shell);
        }
        assert_eq!(TerminalShell::platform_default(), if cfg!(windows) { TerminalShell::Powershell } else if cfg!(target_os = "macos") { TerminalShell::Zsh } else { TerminalShell::Bash });
        assert_eq!(TerminalShell::agent_host_default(), if cfg!(windows) { TerminalShell::Powershell } else { TerminalShell::Bash });
    }

    fn approval_fixture(name:&str)->(SessionManager,SessionRecord,std::path::PathBuf){
        let(path,events)=test_paths(name);let manager=SessionManager::open_at(path.clone(),events).unwrap();
        let record=manager.create(NewSession{provider_id:"fixture".into(),provider_name:"Fixture".into(),cwd:path.parent().unwrap().display().to_string(),yolo:false,auto_mode:false,project_id:None}).unwrap();
        manager.attach_runtime(&record.id,77).unwrap();
        manager.apply_hook_value(&serde_json::json!({"session_id":record.claude_session_id,APP_SESSION_ID_FIELD:record.id,"hook_event_name":"Notification","notification_type":"permission_prompt","message":"approve fixture"})).unwrap();
        let waiting=manager.get(&record.id).unwrap();(manager,waiting,path)
    }

    #[test]
    fn waiting_approval_compare_and_send_consumes_allow_and_deny_once(){
        for allow in [false,true]{
            let(manager,record,path)=approval_fixture("approval-once");let generation=record.waiting_since.unwrap();let writes=std::cell::Cell::new(0);
            let updated=manager.respond_waiting_approval(&record.id,77,generation,allow,|pty|{assert_eq!(pty,77);assert!(manager.get(&record.id).is_some(),"global data must not cross writer");writes.set(writes.get()+1);Ok(())}).unwrap();
            assert_eq!(updated.waiting_since,None);
            assert!(manager.respond_waiting_approval(&record.id,77,generation,allow,|_|{writes.set(writes.get()+1);Ok(())}).is_err());
            assert_eq!(writes.get(),1);fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn waiting_approval_rejects_stale_binding_generation_archived_and_other_runtime(){
        let(manager,record,path)=approval_fixture("approval-cas");let generation=record.waiting_since.unwrap();
        for(pty,expected)in[(78,generation),(77,generation+1)]{assert!(manager.respond_waiting_approval(&record.id,pty,expected,true,|_|panic!("stale approval wrote input")).is_err());}
        manager.archive(&record.id).unwrap();assert!(manager.respond_waiting_approval(&record.id,77,generation,true,|_|panic!("archived approval wrote input")).is_err());
        manager.restore(&record.id).unwrap();
        {let mut data=manager.data.lock().unwrap();let record=data.sessions.iter_mut().find(|item|item.id==record.id).unwrap();record.kind=SessionKind::Terminal;record.agent_runtime_kind=Some("codex".into());}
        assert!(manager.respond_waiting_approval(&record.id,77,generation,true,|_|panic!("non-Claude approval wrote input")).is_err());fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn waiting_generation_is_strictly_monotonic_and_persisted(){
        let(manager,record,path)=approval_fixture("approval-generation");let first=record.waiting_since.unwrap();
        let event=serde_json::json!({"session_id":record.claude_session_id,APP_SESSION_ID_FIELD:record.id,"hook_event_name":"Notification","notification_type":"permission_prompt"});
        manager.apply_hook_value(&event).unwrap();let second=manager.get(&record.id).unwrap().waiting_since.unwrap();assert!(second>first);
        manager.respond_waiting_approval(&record.id,77,second,false,|_|Ok(())).unwrap();drop(manager);
        let reopened=SessionManager::open_at(path.clone(),path.parent().unwrap().join("events")).unwrap();reopened.attach_runtime(&record.id,77).unwrap();reopened.apply_hook_value(&event).unwrap();assert!(reopened.get(&record.id).unwrap().waiting_since.unwrap()>second);fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn approval_input_or_save_failure_does_not_offer_the_same_decision_again(){
        let(manager,record,path)=approval_fixture("approval-write-failed");let generation=record.waiting_since.unwrap();
        assert!(manager.respond_waiting_approval(&record.id,77,generation,true,|_|Err(anyhow!("fixture partial write"))).is_err());
        assert!(manager.respond_waiting_approval(&record.id,77,generation,true,|_|panic!("uncertain input was replayed")).is_err());
        assert!(manager.get(&record.id).unwrap().last_error.unwrap().contains("部分发出"));
        manager.apply_hook_value(&serde_json::json!({"session_id":record.claude_session_id,APP_SESSION_ID_FIELD:record.id,"hook_event_name":"Notification","notification_type":"permission_prompt"})).unwrap();
        let fresh=manager.get(&record.id).unwrap().waiting_since.unwrap();let original=fs::metadata(&path).unwrap().permissions();
        let result=manager.respond_waiting_approval(&record.id,77,fresh,true,|_|{let mut readonly=fs::metadata(&path)?.permissions();readonly.set_readonly(true);fs::set_permissions(&path,readonly)?;Ok(())});
        assert!(result.unwrap_err().to_string().contains("落盘失败"));
        assert!(manager.respond_waiting_approval(&record.id,77,fresh,true,|_|panic!("sent approval replayed after save failure")).is_err());
        fs::set_permissions(&path,original).unwrap();fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn approval_writer_serializes_same_session_hooks_without_holding_global_data(){
        use std::sync::mpsc;use std::time::Duration;
        let(manager,record,path)=approval_fixture("approval-hook-race");let generation=record.waiting_since.unwrap();
        let(entered_tx,entered_rx)=mpsc::channel();let(release_tx,release_rx)=mpsc::channel();
        let writer={let manager=manager.clone();let id=record.id.clone();std::thread::spawn(move||manager.respond_waiting_approval(&id,77,generation,true,|_|{entered_tx.send(()).unwrap();release_rx.recv_timeout(Duration::from_secs(5)).unwrap();Ok(())}))};
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(manager.get(&record.id).unwrap().waiting_since,Some(generation));
        let other=manager.create(NewSession{provider_id:"other".into(),provider_name:"Other".into(),cwd:path.parent().unwrap().display().to_string(),yolo:false,auto_mode:false,project_id:None}).unwrap();
        manager.rename(&other.id,"unrelated data remains available").unwrap();
        let(done_tx,done_rx)=mpsc::channel();
        let hook={let manager=manager.clone();let id=record.id.clone();let native=record.claude_session_id.clone();std::thread::spawn(move||{let result=manager.apply_hook_value(&serde_json::json!({"session_id":native,APP_SESSION_ID_FIELD:id,"hook_event_name":"Notification","notification_type":"permission_prompt"}));done_tx.send(()).unwrap();result})};
        assert!(done_rx.recv_timeout(Duration::from_millis(50)).is_err());
        release_tx.send(()).unwrap();writer.join().unwrap().unwrap();hook.join().unwrap().unwrap();
        assert!(manager.get(&record.id).unwrap().waiting_since.unwrap()>generation);
        assert!(manager.respond_waiting_approval(&record.id,77,generation,true,|_|panic!("old generation wrote into new approval")).is_err());fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn hook_ingestion_and_terminal_submission_do_not_deadlock() {
        use std::sync::{Barrier, mpsc};
        use std::time::Duration;
        for kind in ["codex", "pi"] {
            let (path, events_dir) = test_paths(&format!("concurrent-readiness-{kind}"));
            let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
            let session = manager.create_runtime(NewRuntimeSession { runtime_kind: kind.into(), cwd: path.parent().unwrap().display().to_string(), project_id: None }).unwrap();
            manager.attach_runtime(&session.id, 123).unwrap();
            let token = if kind == "pi" {
                manager.configure_pi_readiness(&mut std::process::Command::new("unused-pi-fixture"), &session.id).unwrap()
            } else { String::new() };
            let barrier = Arc::new(Barrier::new(2));
            let (done, completed) = mpsc::channel();
            let mut workers = Vec::new();
            for submitter in [false, true] {
                let manager = manager.clone(); let session = session.clone(); let token = token.clone();
                let barrier = Arc::clone(&barrier); let done = done.clone();
                workers.push(std::thread::spawn(move || {
                    let result = (|| -> Result<()> {
                        for sequence in 1..=200 {
                            barrier.wait();
                            if submitter {
                                if kind == "pi" { manager.set_runtime_command_host(&session.id, false)?; manager.mark_terminal_submission(123)?; }
                                else { manager.mark_terminal_submission(123)?; }
                            } else {
                                if kind == "pi" { manager.register_agent_prompt(&session.id, "completed fixture prompt"); }
                                let event = if kind == "pi" { serde_json::json!({"hook_event_name":"RuntimePromptState", "session_id":session.id, APP_SESSION_ID_FIELD:session.id, "runtime_kind":"pi", "runtime_token":token, "runtime_sequence":sequence, "ready":true, "turn_completion":{"prompt":"completed fixture prompt","summary":"completed successfully","error":null,"model":"fixture-model"}}) }
                                    else { serde_json::json!({"hook_event_name":"UserPromptSubmit", "session_id":session.claude_session_id, APP_SESSION_ID_FIELD:session.id, "prompt":"concurrent fixture"}) };
                                manager.apply_hook_value(&event)?;
                            }
                        }
                        Ok(())
                    })();
                    done.send(result).unwrap();
                }));
            }
            for _ in 0..2 { completed.recv_timeout(Duration::from_secs(15)).expect("hook/input lock order deadlocked").unwrap(); }
            for worker in workers { worker.join().unwrap(); }
            if kind == "pi" { assert!(!manager.drain_summary_requests().is_empty(), "successful Pi completion did not reach title queue"); }
            manager.set_runtime_command_host(&session.id, false).unwrap();
            manager.mark_prompt_dispatched(&session.id);
            assert!(!manager.is_ready_for_prompt(&session.id));
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn session_publication_failure_preserves_the_previous_valid_file() {
        let (path, _) = test_paths("atomic-session-publish");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = SessionsFile::default();
        save_sessions_at(&path, &original).unwrap();
        let before = fs::read(&path).unwrap();
        let mut changed = SessionsFile::default(); changed.generated_titles.insert("not-published".into());
        let result = save_sessions_with(&path, &changed, |_, _| Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "fixture rename failure")));
        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(serde_json::from_slice::<SessionsFile>(&before).is_ok());
        assert!(!fs::read_dir(path.parent().unwrap()).unwrap().any(|entry| entry.unwrap().path().extension().is_some_and(|value| value == "tmp")));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn concurrent_readers_only_observe_complete_session_files() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let (path, _) = test_paths("atomic-session-readers");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        save_sessions_at(&path, &SessionsFile::default()).unwrap();
        let finished = Arc::new(AtomicBool::new(false));
        let reader_path = path.clone(); let reader_finished = Arc::clone(&finished);
        let reader = std::thread::spawn(move || {
            let mut reads = 0;
            while !reader_finished.load(Ordering::Acquire) {
                let bytes = fs::read(&reader_path).unwrap();
                serde_json::from_slice::<SessionsFile>(&bytes).expect("session reader observed a partial JSON file");
                reads += 1;
            }
            reads
        });
        for index in 0..50 {
            let mut data = SessionsFile::default(); data.generated_titles.insert(format!("{index}-{}", "并发配置".repeat(500)));
            save_sessions_at(&path, &data).unwrap();
        }
        finished.store(true, Ordering::Release);
        assert!(reader.join().unwrap() > 0);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn runtime_title_requests_persist_success_and_respect_manual_deleted_and_stale_sessions() {
        let (path, events_dir) = test_paths("runtime-titles");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        for kind in ["claude_chat", "codex_chat", "kimi_chat", "opencode_chat", "pi_chat", "deepseek_harness", "claude_code", "codex", "kimi_code", "pi", "opencode"] {
            let record = manager.create_runtime(NewRuntimeSession { runtime_kind: kind.into(), cwd: "F:\\project\\demo".into(), project_id: None }).unwrap();
            manager.queue_title_summary_for_model(&record.id, "修复登录接口超时", Some("native-id".into()), Some("selected-model".into()));
            manager.queue_title_summary_for_model(&record.id, "第二条消息不应重复派发", None, None);
            let requests = manager.drain_summary_requests();
            assert_eq!(requests.len(), 1, "{kind}");
            assert_eq!(requests[0].model.as_deref(), Some("selected-model"));
            let mut stale = requests[0].clone();
            stale.cwd = "F:\\other".into();
            assert!(manager.apply_title_request(&stale, "错误摘要").unwrap().is_none());
            assert_eq!(manager.get(&record.id).unwrap().title, record.title);
            manager.apply_title_request(&requests[0], "登录超时修复").unwrap().unwrap();
            let reopened = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
            reopened.queue_title_summary_for_model(&record.id, "重启不得重新生成", None, None);
            assert!(reopened.drain_summary_requests().is_empty());
            manager.rename(&record.id, "手动名称").unwrap();
            assert!(manager.apply_title_request(&requests[0], "迟到的摘要").unwrap().is_none());
            manager.delete(&record.id).unwrap();
            assert!(manager.apply_title_request(&requests[0], "已删除摘要").unwrap().is_none());
        }
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    use super::*;

    #[test]
    fn brand_migration_ingests_legacy_hooks_without_losing_identity_or_chronology() {
        let (path, events) = test_paths("brand-old-hook");
        let manager = SessionManager::open_at(path.clone(), events.clone()).unwrap();
        let record = manager.create_runtime(NewRuntimeSession { runtime_kind: "claude_code".into(), cwd: ".".into(), project_id: None }).unwrap();
        manager.attach_runtime(&record.id, 33).unwrap();
        fs::write(events.join("legacy.json"), serde_json::to_vec(&serde_json::json!({
            "session_id": record.claude_session_id, crate::brand::LEGACY_APP_SESSION: record.id,
            crate::brand::LEGACY_QUEUED_AT: 17, "hook_event_name": "SessionStart",
        })).unwrap()).unwrap();
        assert_eq!(manager.ingest_events().unwrap(), 1);
        assert!(manager.is_ready_for_prompt(&record.id));
        let conflict = events.join("conflict.json");
        fs::write(&conflict, serde_json::to_vec(&serde_json::json!({
            "session_id": record.claude_session_id, APP_SESSION_ID_FIELD: record.id,
            crate::brand::LEGACY_APP_SESSION: "another-session", "hook_event_name": "SessionStart",
        })).unwrap()).unwrap();
        assert!(manager.ingest_events().is_err());
        assert!(conflict.exists(), "conflicting source must remain available for diagnosis");
        assert!(manager.is_ready_for_prompt(&record.id));
        assert!(path.parent().unwrap().canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    use std::fs;

    fn test_paths(name: &str) -> (std::path::PathBuf, std::path::PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "crewvela-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        (root.join("sessions.json"), root.join("events"))
    }

    fn fail_session_write<T>(manager: &SessionManager, operation: impl FnOnce() -> Result<T>) {
        let memory_before = serde_json::to_value(&*manager.data.lock().unwrap()).unwrap();
        let disk_before = fs::read(&manager.path).unwrap();
        let permissions = fs::metadata(&manager.path).unwrap().permissions();
        let mut readonly = permissions.clone();
        readonly.set_readonly(true);
        fs::set_permissions(&manager.path, readonly).unwrap();
        let result = operation();
        fs::set_permissions(&manager.path, permissions).unwrap();
        assert!(result.is_err(), "read-only session storage must reject changes");
        assert_eq!(serde_json::to_value(&*manager.data.lock().unwrap()).unwrap(), memory_before,
            "failed persistence must not publish the candidate in memory");
        assert_eq!(fs::read(&manager.path).unwrap(), disk_before);
    }

    #[test]
    fn session_create_failures_leave_no_phantom_records_and_allow_retry() {
        for kind in ["claude", "terminal", "runtime", "agent"] {
            let (path, events) = test_paths(&format!("create-failure-{kind}"));
            let manager = SessionManager::open_at(path.clone(), events.clone()).unwrap();
            manager.save(&manager.data.lock().unwrap()).unwrap();
            let create = || match kind {
                "claude" => manager.create(NewSession {
                    provider_id: "fixture".into(), provider_name: "Fixture".into(), cwd: ".".into(),
                    yolo: false, auto_mode: false, project_id: None,
                }),
                "terminal" => manager.create_terminal(NewTerminalSession {
                    shell: TerminalShell::platform_default(), cwd: ".".into(), project_id: None,
                }),
                "runtime" => manager.create_runtime(NewRuntimeSession {
                    runtime_kind: "codex".into(), cwd: ".".into(), project_id: None,
                }),
                _ => manager.create_agent(NewAgentSession {
                    avatar_id: None, project_id: "project".into(), agent_id: "agent".into(),
                    runtime_kind: "codex".into(), agent_name: "Fixture".into(), cwd: ".".into(),
                }),
            };
            fail_session_write(&manager, create);
            assert!(manager.list().is_empty());
            assert!(SessionManager::open_at(path.clone(), events.clone()).unwrap().list().is_empty());
            let created = create().unwrap();
            assert_eq!(manager.list().len(), 1);
            assert!(SessionManager::open_at(path.clone(), events).unwrap().get(&created.id).is_some());
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn session_management_failures_preserve_records_and_allow_retry() {
        for operation in ["rename", "archive", "restore", "archive-project", "clear-project", "worktree", "approval", "handoff", "retire", "profile", "command-host"] {
            let (path, events) = test_paths(&format!("management-failure-{operation}"));
            let manager = SessionManager::open_at(path.clone(), events).unwrap();
            let record = manager.create_agent(NewAgentSession {
                avatar_id: None, project_id: "project".into(), agent_id: "agent".into(),
                runtime_kind: "codex".into(), agent_name: "Fixture".into(), cwd: ".".into(),
            }).unwrap();
            {
                let mut data = manager.data.lock().unwrap();
                data.sessions.iter_mut().find(|session| session.id == record.id).unwrap().project_id = Some("project".into());
                manager.save(&data).unwrap();
            }
            if operation == "restore" { manager.archive(&record.id).unwrap(); }
            manager.ready_for_prompt.lock().unwrap().insert(record.id.clone());
            manager.runtime_prompt_buffers.lock().unwrap().insert(record.id.clone(), "prompt".into());
            let change = || -> Result<()> {
                match operation {
                    "rename" => { manager.rename(&record.id, "Renamed")?; }
                    "archive" => { manager.archive(&record.id)?; }
                    "restore" => { manager.restore(&record.id)?; }
                    "archive-project" => { manager.archive_project_sessions("project")?; }
                    "clear-project" => { manager.clear_project("project")?; }
                    "worktree" => { manager.assign_worktree(&record.id, SessionWorktree {
                        repo_root: ".".into(), path: "isolated-worktree".into(), branch: "task".into(),
                    })?; }
                    "approval" => { manager.set_approval_policy(&record.id, ApprovalPolicy::Deny)?; }
                    "handoff" => { manager.set_handoff_plan(&record.id)?; }
                    "retire" => { manager.retire_agent_runtime_sessions("project", "agent")?; }
                    "command-host" => { manager.set_runtime_command_host(&record.id, true)?; }
                    "profile" => { manager.sync_agent_profile(&serde_json::from_value(serde_json::json!({
                        "id": "agent", "projectId": "project", "name": "Updated member", "runtimeKind": "codex",
                        "role": "worker", "description": "Fixture", "permissionProfile": "default",
                        "maxConcurrency": 1, "enabled": true, "createdAt": 1, "updatedAt": 2,
                    }))?)?; }
                    _ => unreachable!(),
                }
                Ok(())
            };
            fail_session_write(&manager, change);
            assert!(manager.is_ready_for_prompt(&record.id), "failed {operation} cleared prompt readiness");
            assert!(manager.runtime_prompt_buffers.lock().unwrap().contains_key(&record.id));
            change().unwrap();
            if matches!(operation, "retire" | "command-host") {
                assert!(!manager.is_ready_for_prompt(&record.id));
                assert!(manager.runtime_prompt_buffers.lock().unwrap().is_empty());
            }
            assert_eq!(serde_json::from_slice::<serde_json::Value>(&fs::read(&path).unwrap()).unwrap(),
                serde_json::to_value(&*manager.data.lock().unwrap()).unwrap());
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn session_delete_failure_preserves_auxiliary_state_and_allows_retry() {
        let (path, events) = test_paths("delete-failure");
        let manager = SessionManager::open_at(path.clone(), events.clone()).unwrap();
        let record = manager.create_terminal(NewTerminalSession {
            shell: TerminalShell::platform_default(), cwd: ".".into(), project_id: None,
        }).unwrap();
        manager.ready_for_prompt.lock().unwrap().insert(record.id.clone());
        manager.runtime_prompt_buffers.lock().unwrap().insert(record.id.clone(), "prompt".into());
        manager.pending_first_prompts.lock().unwrap().insert(record.id.clone(), "first prompt".into());
        manager.summarized_sessions.lock().unwrap().insert(record.id.clone());
        manager.active_agent_prompts.lock().unwrap().insert(record.id.clone(), "active".into());
        manager.record_agent_turn_completion(&record.id, "summary".into(), "prompt".into());
        manager.summary_requests.lock().unwrap().push(SummaryRequest {
            session_id: record.id.clone(), provider_id: "fixture".into(), prompt: "prompt".into(),
            runtime_kind: None, cwd: ".".into(), native_session_id: String::new(), upstream_locator: None, model: None,
        });
        {
            let mut data = manager.data.lock().unwrap();
            data.generated_titles.insert(record.id.clone());
            manager.save(&data).unwrap();
        }
        fail_session_write(&manager, || manager.delete(&record.id));
        assert!(manager.get(&record.id).is_some());
        assert!(manager.is_ready_for_prompt(&record.id));
        assert!(manager.runtime_prompt_buffers.lock().unwrap().contains_key(&record.id));
        assert!(manager.pending_first_prompts.lock().unwrap().contains_key(&record.id));
        assert!(manager.summarized_sessions.lock().unwrap().contains(&record.id));
        assert!(manager.active_agent_prompts.lock().unwrap().contains_key(&record.id));
        assert_eq!(manager.agent_turn_revision(&record.id), 1);
        assert_eq!(manager.summary_requests.lock().unwrap().len(), 1);
        assert!(SessionManager::open_at(path.clone(), events.clone()).unwrap().get(&record.id).is_some());
        manager.delete(&record.id).unwrap();
        assert!(manager.get(&record.id).is_none());
        assert!(!manager.is_ready_for_prompt(&record.id));
        assert!(manager.runtime_prompt_buffers.lock().unwrap().is_empty());
        assert!(manager.pending_first_prompts.lock().unwrap().is_empty());
        assert!(manager.summarized_sessions.lock().unwrap().is_empty());
        assert!(manager.active_agent_prompts.lock().unwrap().is_empty());
        assert_eq!(manager.agent_turn_revision(&record.id), 0);
        assert!(manager.summary_requests.lock().unwrap().is_empty());
        assert!(SessionManager::open_at(path.clone(), events).unwrap().get(&record.id).is_none());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn agent_avatar_session_legacy_and_retired_history_are_preserved() {
        let root = std::env::temp_dir().join(format!("cc-avatar-session-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let old = manager
            .create_agent(NewAgentSession {
                avatar_id: Some("orbit".into()),
                project_id: "project".into(),
                agent_id: "agent".into(),
                runtime_kind: "codex".into(),
                agent_name: "member".into(),
                cwd: ".".into(),
            })
            .unwrap();
        let mut legacy = serde_json::to_value(&old).unwrap();
        legacy.as_object_mut().unwrap().remove("agentAvatarId");
        assert_eq!(
            serde_json::from_value::<SessionRecord>(legacy)
                .unwrap()
                .agent_avatar_id,
            None
        );
        manager
            .retire_agent_runtime_sessions("project", "agent")
            .unwrap();
        let fresh = manager
            .create_agent(NewAgentSession {
                avatar_id: Some("sprout".into()),
                project_id: "project".into(),
                agent_id: "agent".into(),
                runtime_kind: "pi".into(),
                agent_name: "member".into(),
                cwd: ".".into(),
            })
            .unwrap();
        assert_eq!(fresh.agent_avatar_id.as_deref(), Some("sprout"));
        assert_eq!(
            manager.get(&old.id).unwrap().agent_avatar_id.as_deref(),
            Some("orbit")
        );
        assert!(manager
            .create_agent(NewAgentSession {
                avatar_id: Some("unknown".into()),
                project_id: "project".into(),
                agent_id: "bad".into(),
                runtime_kind: "pi".into(),
                agent_name: "member".into(),
                cwd: ".".into()
            })
            .is_err());
        drop(manager);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_switch_retires_history_and_switching_back_creates_a_fresh_session() {
        let (path, events) = test_paths("runtime-switch-history");
        let manager = SessionManager::open_at(path.clone(), events).unwrap();
        let create = |runtime: &str| NewAgentSession {
            avatar_id: None,
            project_id: "project".into(),
            agent_id: "agent".into(),
            runtime_kind: runtime.into(),
            agent_name: "成员".into(),
            cwd: "root".into(),
        };
        let original = manager.create_agent(create("codex")).unwrap();
        manager
            .retire_agent_runtime_sessions("project", "agent")
            .unwrap();
        let pi = manager.create_agent(create("pi")).unwrap();
        assert_ne!(pi.id, original.id);
        assert_eq!(pi.agent_runtime_kind.as_deref(), Some("pi"));
        for event_name in ["Stop", "AgentTurnComplete", "SessionStart"] {
            assert!(manager.apply_hook_value(&serde_json::json!({"session_id": original.id, APP_SESSION_ID_FIELD:original.id, "hook_event_name":event_name})).unwrap().is_none());
        }
        let old = manager.get(&original.id).unwrap();
        assert!(old.retired_agent_runtime);
        assert!(old.archived_at.is_some());
        assert_eq!(old.title, original.title);
        assert_eq!(old.agent_runtime_kind, original.agent_runtime_kind);
        assert_eq!(old.multi_agent_agent_id, original.multi_agent_agent_id);
        assert_eq!(manager.list().len(), 1);
        assert_eq!(manager.list_archived().len(), 1);
        manager.attach_runtime(&pi.id, 99).unwrap();
        let mut command = std::process::Command::new("pi");
        let token = manager
            .configure_pi_readiness(&mut command, &pi.id)
            .unwrap();
        manager
            .retire_agent_runtime_sessions("project", "agent")
            .unwrap();
        assert!(manager.apply_hook_value(&serde_json::json!({"session_id":pi.id,APP_SESSION_ID_FIELD:pi.id,"hook_event_name":"RuntimePromptState","runtime_kind":"pi","runtime_token":token,"runtime_sequence":1,"ready":true})).unwrap().is_none());
        assert_eq!(manager.get(&pi.id).unwrap().status, SessionStatus::Stopped);
        let switched_back = manager.create_agent(create("codex")).unwrap();
        assert_ne!(switched_back.id, original.id);
        assert_ne!(switched_back.id, pi.id);
        assert!(switched_back.claude_session_id.is_empty());
        assert!(switched_back.pty_id.is_none());
        assert_eq!(manager.list_archived().len(), 2);
        manager.restore(&original.id).unwrap();
        assert!(manager.get(&original.id).unwrap().retired_agent_runtime);
        assert_eq!(
            manager.create_agent(create("codex")).unwrap().id,
            switched_back.id
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn member_profile_rename_keeps_session_identity_and_native_history() {
        let (path, events) = test_paths("member-rename");
        let manager = SessionManager::open_at(path.clone(), events).unwrap();
        let original = manager
            .create_agent(NewAgentSession {
                avatar_id: None,
                project_id: "project".into(),
                agent_id: "agent".into(),
                runtime_kind: "pi".into(),
                agent_name: "旧名字".into(),
                cwd: "project-root".into(),
            })
            .unwrap();
        let profile = crate::multi_agent::AgentProfile {
            avatar_id: None,
            id: "agent".into(),
            project_id: "project".into(),
            name: "接口审查".into(),
            runtime_kind: crate::multi_agent::AgentRuntimeKind::Pi,
            role: crate::multi_agent::AgentRole::Reviewer,
            description: "只读审查".into(),
            persona: Default::default(),
            persona_custom: String::new(),
            model: Some("next-model".into()),
            permission_profile: crate::multi_agent::PermissionProfile::Default,
            max_concurrency: 1,
            capabilities: vec![],
            enabled: true,
            created_at: 0,
            updated_at: 0,
        };
        manager.sync_agent_profile(&profile).unwrap();
        let updated = manager.get(&original.id).unwrap();
        assert_eq!(updated.id, original.id);
        assert_eq!(updated.title, "Pi · 接口审查");
        assert_eq!(updated.provider_name, updated.title);
        assert_eq!(updated.cwd, original.cwd);
        assert_eq!(updated.claude_session_id, original.claude_session_id);
        assert_eq!(updated.agent_runtime_kind, original.agent_runtime_kind);
        assert_eq!(updated.multi_agent_agent_id, original.multi_agent_agent_id);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn removing_an_already_consumed_event_is_idempotent() {
        let missing = std::env::temp_dir().join(format!(
            "crewvela-missing-event-{}.json",
            uuid::Uuid::new_v4()
        ));
        remove_ingested_event(&missing).unwrap();
    }

    #[test]
    fn archived_session_is_hidden_until_restored_and_keeps_project() {
        let (path, events_dir) = test_paths("archive-session");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: Some("project-a".into()),
            })
            .unwrap();

        let archived = manager.archive(&created.id).unwrap();
        assert!(archived.archived_at.is_some());
        assert!(manager.list().is_empty());
        assert_eq!(manager.list_archived().len(), 1);
        assert_eq!(
            manager.list_archived()[0].project_id.as_deref(),
            Some("project-a")
        );

        drop(manager);
        let reopened = SessionManager::open_at(path.clone(), events_dir).unwrap();
        assert!(reopened.list().is_empty());
        assert_eq!(reopened.list_archived().len(), 1);

        let restored = reopened.restore(&created.id).unwrap();
        assert!(restored.archived_at.is_none());
        assert_eq!(reopened.list().len(), 1);
        assert!(reopened.list_archived().is_empty());
        assert_eq!(restored.project_id.as_deref(), Some("project-a"));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn equal_session_timestamps_preserve_persisted_order_after_reopen() {
        let (path, events_dir) = test_paths("equal-session-timestamps");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let mut expected = Vec::new();
        for title in ["归档 A", "归档 B", "活动 A", "活动 B"] {
            let created = manager.create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            }).unwrap();
            manager.rename(&created.id, title).unwrap();
            expected.push(created.id);
        }
        {
            let mut data = manager.data.lock().unwrap();
            for (index, session) in data.sessions.iter_mut().enumerate() {
                session.updated_at = 100;
                session.archived_at = (index < 2).then_some(50);
            }
            manager.save(&data).unwrap();
        }
        let ids = |records: Vec<SessionRecord>| records.into_iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(ids(manager.list()), expected[2..]);
        assert_eq!(ids(manager.list_archived()), expected[..2]);
        drop(manager);
        let reopened = SessionManager::open_at(path.clone(), events_dir).unwrap();
        assert_eq!(ids(reopened.list()), expected[2..]);
        assert_eq!(ids(reopened.list_archived()), expected[..2]);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn archiving_project_sessions_only_hides_matching_project_chats() {
        let (path, events_dir) = test_paths("archive-project-sessions");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        for (sequence, (title, project_id)) in [
            ("项目 A-1", Some("project-a")),
            ("项目 A-2", Some("project-a")),
            ("项目 B", Some("project-b")),
            ("未分组", None),
        ].into_iter().enumerate() {
            let created = manager
                .create(NewSession {
                    provider_id: "glm".into(),
                    provider_name: "Z.AI GLM".into(),
                    cwd: "F:\\project\\demo".into(),
                    yolo: false,
                    auto_mode: false,
                    project_id: project_id.map(str::to_string),
                })
                .unwrap();
            manager.rename(&created.id, title).unwrap();
            // The archive contract is independent of same-millisecond creation ties.
            manager.data.lock().unwrap().sessions.iter_mut().find(|session| session.id == created.id).unwrap().updated_at = sequence as u64;
        }

        assert_eq!(manager.archive_project_sessions("project-a").unwrap(), 2);
        assert_eq!(
            manager
                .list()
                .iter()
                .map(|session| session.title.as_str())
                .collect::<Vec<_>>(),
            vec!["未分组", "项目 B"]
        );
        assert_eq!(
            manager
                .list_archived()
                .iter()
                .map(|session| session.title.as_str())
                .collect::<std::collections::HashSet<_>>(),
            std::collections::HashSet::from(["项目 A-1", "项目 A-2"])
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn assign_worktree_switches_cwd_and_persists_metadata() {
        let (path, events_dir) = test_paths("assign-worktree");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        assert!(created.worktree.is_none());

        let worktree = crate::worktree::SessionWorktree {
            repo_root: "F:\\project\\demo".into(),
            path: "C:\\ConfigDir\\worktrees\\sessions\\abc".into(),
            branch: "crewvela/session-abcdef12".into(),
        };
        let updated = manager
            .assign_worktree(&created.id, worktree.clone())
            .unwrap();
        assert_eq!(updated.cwd, worktree.path);
        assert_eq!(updated.worktree, Some(worktree.clone()));
        drop(manager);

        let reopened = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let restored = reopened.get(&created.id).unwrap();
        assert_eq!(restored.cwd, worktree.path);
        assert_eq!(restored.worktree, Some(worktree));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn clear_project_ungroups_only_that_project_sessions() {
        let (path, events_dir) = test_paths("clear-project");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let in_project = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: Some("project-a".into()),
            })
            .unwrap();
        let other_project = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: Some("project-b".into()),
            })
            .unwrap();
        let ungrouped = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();

        assert_eq!(manager.clear_project("project-a").unwrap(), 1);
        assert_eq!(manager.get(&in_project.id).unwrap().project_id, None);
        assert_eq!(
            manager
                .get(&other_project.id)
                .unwrap()
                .project_id
                .as_deref(),
            Some("project-b")
        );
        assert_eq!(manager.get(&ungrouped.id).unwrap().project_id, None);
        // 重复清理是幂等空操作。
        assert_eq!(manager.clear_project("project-a").unwrap(), 0);
        drop(manager);

        let reopened = SessionManager::open_at(path.clone(), events_dir).unwrap();
        assert_eq!(reopened.get(&in_project.id).unwrap().project_id, None);
        assert_eq!(
            reopened
                .get(&other_project.id)
                .unwrap()
                .project_id
                .as_deref(),
            Some("project-b")
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn title_from_prompt_collapses_whitespace_and_truncates() {
        let title =
            title_from_prompt("  请帮我   修复登录接口中偶发的超时问题，并补充相关的回归测试  ");

        assert_eq!(title, "请帮我 修复登录接口中偶发的超时问题，并补充相关的…");
    }

    #[test]
    fn created_session_survives_store_reopen() {
        let (path, events_dir) = test_paths("persist");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();

        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: true,
                project_id: None,
            })
            .unwrap();
        drop(manager);

        let reopened = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let sessions = reopened.list();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, created.id);
        assert_eq!(sessions[0].status, SessionStatus::Pending);
        assert_eq!(sessions[0].title, "新会话");
        assert!(sessions[0].auto_mode);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn delete_all_removes_every_persisted_session() {
        let (path, events_dir) = test_paths("delete-all");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let first = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        let second = manager
            .create_terminal(NewTerminalSession {
                shell: TerminalShell::Powershell,
                cwd: "F:\\project\\demo".into(),
                project_id: None,
            })
            .unwrap();

        manager.delete(&first.id).unwrap();
        manager.delete(&second.id).unwrap();
        assert!(manager.list().is_empty());
        drop(manager);

        let reopened = SessionManager::open_at(path.clone(), events_dir).unwrap();
        assert!(reopened.list().is_empty());

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn terminal_session_survives_store_reopen_with_shell_type() {
        let (path, events_dir) = test_paths("terminal-persist");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();

        let created = manager
            .create_terminal(NewTerminalSession {
                shell: TerminalShell::Powershell,
                cwd: "F:\\project\\demo".into(),
                project_id: None,
            })
            .unwrap();
        assert_eq!(created.kind, SessionKind::Terminal);
        assert_eq!(created.shell, Some(TerminalShell::Powershell));
        assert_eq!(created.provider_name, "PowerShell");
        drop(manager);

        let reopened = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let restored = reopened.list().pop().unwrap();
        assert_eq!(restored.kind, SessionKind::Terminal);
        assert_eq!(restored.shell, Some(TerminalShell::Powershell));
        assert_eq!(restored.cwd, "F:\\project\\demo");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn hook_events_drive_status_and_generate_title_from_first_prompt() {
        let (path, events_dir) = test_paths("hooks");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "UserPromptSubmit",
                "prompt": "修复用户登录超时并补充回归测试"
            }))
            .unwrap();
        let running = manager.get(&created.id).unwrap();
        assert_eq!(running.status, SessionStatus::Running);
        assert_eq!(running.title, "修复用户登录超时并补充回归测试");

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": running.claude_session_id,
                "hook_event_name": "Stop"
            }))
            .unwrap();
        assert_eq!(
            manager.get(&created.id).unwrap().status,
            SessionStatus::Completed
        );

        let requests = manager.drain_summary_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].session_id, created.id);
        assert_eq!(requests[0].provider_id, "glm");
        assert_eq!(requests[0].prompt, "修复用户登录超时并补充回归测试");
        // 取走一次后不会重复派发。
        assert!(manager.drain_summary_requests().is_empty());

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn summary_request_is_queued_only_once_per_session() {
        let (path, events_dir) = test_paths("summary-once");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();

        for prompt in ["第一条消息", "第二条消息"] {
            manager
                .apply_hook_value(&serde_json::json!({
                    "session_id": created.claude_session_id,
                    "hook_event_name": "UserPromptSubmit",
                    "prompt": prompt
                }))
                .unwrap();
            manager
                .apply_hook_value(&serde_json::json!({
                    "session_id": created.claude_session_id,
                    "hook_event_name": "Stop"
                }))
                .unwrap();
        }

        let requests = manager.drain_summary_requests();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].prompt, "第一条消息");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn multi_agent_session_keeps_visual_runtime_identity() {
        let (path, events_dir) = test_paths("multi-agent-session");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create_agent(NewAgentSession {
                avatar_id: None,
                project_id: "project-1".into(),
                agent_id: "agent-codex".into(),
                runtime_kind: "codex".into(),
                agent_name: "后端开发".into(),
                cwd: "F:\\project\\demo".into(),
            })
            .unwrap();

        assert_eq!(created.kind, SessionKind::Terminal);
        assert_eq!(created.provider_name, "Codex · 后端开发");
        assert_eq!(created.multi_agent_project_id.as_deref(), Some("project-1"));
        assert_eq!(created.multi_agent_agent_id.as_deref(), Some("agent-codex"));
        assert_eq!(created.agent_runtime_kind.as_deref(), Some("codex"));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn failed_member_activity_persistence_keeps_status_and_does_not_publish_event() {
        let (path, events) = test_paths("member-event-save-failure");
        let sessions = SessionManager::open_at(path.clone(), events).unwrap();
        let record = sessions.create_agent(NewAgentSession { avatar_id: None, project_id: "project".into(), agent_id: "member".into(), runtime_kind: "pi".into(), agent_name: "Pi".into(), cwd: ".".into() }).unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(sessions.mark_agent_activity(&record.id, SessionStatus::Error, Some("failed".into())).is_err());
        assert_eq!(sessions.get(&record.id).unwrap().status, record.status);
        assert!(sessions.drain_transitions().is_empty());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn unknown_runtime_history_is_readable_but_cannot_start_or_deliver() {
        let (path, events) = test_paths("unknown-runtime-history");
        let manager = SessionManager::open_at(path.clone(), events.clone()).unwrap();
        let record = manager.create_runtime(NewRuntimeSession { runtime_kind: "pi".into(), cwd: ".".into(), project_id: None }).unwrap();
        let mut saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        saved["sessions"][0]["agentRuntimeKind"] = serde_json::json!("removed_runtime");
        saved["sessions"][0]["status"] = serde_json::json!("running");
        saved["sessions"][0]["ptyId"] = serde_json::json!(123);
        fs::write(&path, serde_json::to_vec(&saved).unwrap()).unwrap();
        let reopened = SessionManager::open_at(path.clone(), events).unwrap();
        let historical = reopened.get(&record.id).unwrap();
        assert_eq!(historical.agent_runtime_kind.as_deref(), Some("removed_runtime"));
        assert!(historical.retired_agent_runtime);
        assert_eq!(historical.status, SessionStatus::Stopped);
        assert!(historical.pty_id.is_none());
        assert!(reopened.begin_runtime_start(&record.id).is_err());
        assert!(reopened.queue_runtime_delivery(&record.id, "message", "hello").is_err());
        assert!(reopened.create_runtime(NewRuntimeSession { runtime_kind: "removed_runtime".into(), cwd: ".".into(), project_id: None }).is_err());
        assert_eq!(load_records(&path).unwrap()[0].id, record.id);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn removed_cli_history_is_retired_without_losing_its_identity() {
        let (path, events_dir) = test_paths("retired-cli");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let session = manager.create_runtime(NewRuntimeSession { runtime_kind: "codex".into(), cwd: ".".into(), project_id: None }).unwrap();
        drop(manager);
        let mut saved: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        saved["sessions"][0]["agentRuntimeKind"] = serde_json::json!("minimax_code");
        saved["sessions"][0]["ptyId"] = serde_json::json!(42);
        saved["sessions"][0]["status"] = serde_json::json!("running");
        fs::write(&path,serde_json::to_vec(&saved).unwrap()).unwrap();
        let readonly = load_records(&path).unwrap();
        assert!(readonly[0].retired_agent_runtime);
        assert!(readonly[0].pty_id.is_none());
        let manager = SessionManager::open_at(path.clone(),events_dir).unwrap();
        let retired = manager.get(&session.id).unwrap();
        assert_eq!(retired.agent_runtime_kind.as_deref(), Some("minimax_code"));
        assert!(retired.retired_agent_runtime);
        assert_eq!(retired.status,SessionStatus::Stopped);
        assert!(retired.pty_id.is_none());
        assert!(manager.create_runtime(NewRuntimeSession { runtime_kind: "minimax_code".into(), cwd: ".".into(), project_id: None }).is_err());
        drop(manager);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn standalone_session_supports_every_interactive_runtime() {
        let (path, events_dir) = test_paths("standalone-runtimes");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();

        for (runtime_kind, runtime_label) in [
            ("codex", "Codex CLI"),
            ("kimi_code", "Kimi Code"),
            ("pi", "Pi Agent"),
            ("opencode", "OpenCode"),
            ("zcode_cli", "ZCode CLI"),
        ] {
            let created = manager
                .create_runtime(NewRuntimeSession {
                    runtime_kind: runtime_kind.into(),
                    cwd: "F:\\project\\demo".into(),
                    project_id: None,
                })
                .unwrap();

            assert_eq!(created.kind, SessionKind::Terminal);
            assert_eq!(created.agent_runtime_kind.as_deref(), Some(runtime_kind));
            assert_eq!(created.title, runtime_label);
            assert_eq!(created.provider_name, runtime_label);
        }

        assert!(manager
            .create_runtime(NewRuntimeSession {
                runtime_kind: "oh_my_pi".into(),
                cwd: "F:\\project\\demo".into(),
                project_id: None,
            })
            .unwrap_err()
            .to_string()
            .contains("已移除"));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn historical_removed_runtime_session_can_still_be_loaded() {
        let (path, events_dir) = test_paths("historical-removed-runtime");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let created = manager
            .create_runtime(NewRuntimeSession {
                runtime_kind: "pi".into(),
                cwd: "F:\\project\\demo".into(),
                project_id: None,
            })
            .unwrap();
        drop(manager);
        let mut data: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let session = data["sessions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|item| item["id"] == created.id)
            .unwrap();
        session["agentRuntimeKind"] = serde_json::json!("oh_my_pi");
        session["providerName"] = serde_json::json!("Oh My Pi");
        fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        let reopened = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let historical = reopened.get(&created.id).unwrap();
        assert_eq!(historical.agent_runtime_kind.as_deref(), Some("oh_my_pi"));
        assert_eq!(historical.provider_name, "Oh My Pi");
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn summary_request_is_skipped_for_locked_title() {
        let (path, events_dir) = test_paths("summary-locked");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.rename(&created.id, "手动标题").unwrap();

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "UserPromptSubmit",
                "prompt": "不应触发摘要"
            }))
            .unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Stop"
            }))
            .unwrap();

        assert!(manager.drain_summary_requests().is_empty());

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn apply_summary_title_respects_manual_rename() {
        let (path, events_dir) = test_paths("summary-apply");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();

        let updated = manager
            .apply_summary_title(&created.id, "登录超时修复")
            .unwrap();
        assert_eq!(updated.unwrap().title, "登录超时修复");

        manager.rename(&created.id, "手动标题").unwrap();
        let skipped = manager
            .apply_summary_title(&created.id, "登录超时修复")
            .unwrap();
        assert!(skipped.is_none());
        assert_eq!(manager.get(&created.id).unwrap().title, "手动标题");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn hook_events_track_current_permission_mode() {
        let (path, events_dir) = test_paths("permission-mode");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "UserPromptSubmit",
                "permission_mode": "auto",
                "prompt": "检查权限模式"
            }))
            .unwrap();

        assert_eq!(
            manager.get(&created.id).unwrap().permission_mode,
            Some(PermissionMode::Auto)
        );
        assert_eq!(
            manager.get(&created.id).unwrap().permission_mode_revision,
            1
        );

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Stop",
                "permission_mode": "auto"
            }))
            .unwrap();
        assert_eq!(
            manager.get(&created.id).unwrap().permission_mode_revision,
            2
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn permission_mode_prediction_preserves_a_newer_hook_revision() {
        let (path, events_dir) = test_paths("permission-mode-prediction");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart",
                "permission_mode": "default"
            }))
            .unwrap();
        let stale_revision = manager.get(&created.id).unwrap().permission_mode_revision;
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Stop",
                "permission_mode": "auto"
            }))
            .unwrap();

        assert!(!manager
            .predict_permission_mode_if_revision(&created.id, PermissionMode::Plan, stale_revision)
            .unwrap());
        assert_eq!(
            manager.get(&created.id).unwrap().permission_mode,
            Some(PermissionMode::Auto)
        );

        let current_revision = manager.get(&created.id).unwrap().permission_mode_revision;
        assert!(manager
            .predict_permission_mode_if_revision(
                &created.id,
                PermissionMode::Plan,
                current_revision
            )
            .unwrap());
        assert_eq!(
            manager.get(&created.id).unwrap().permission_mode,
            Some(PermissionMode::Plan)
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn yolo_runtime_waits_for_authoritative_hook_mode() {
        let (path, events_dir) = test_paths("yolo-permission-mode");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: true,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();

        assert_eq!(created.permission_mode, None);
        let attached = manager.attach_runtime(&created.id, 9).unwrap();
        assert_eq!(attached.permission_mode, None);

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart",
                "permission_mode": "bypassPermissions"
            }))
            .unwrap();
        assert_eq!(
            manager.get(&created.id).unwrap().permission_mode,
            Some(PermissionMode::BypassPermissions)
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn hook_command_queues_event_and_store_ingests_it() {
        let (path, events_dir) = test_paths("event-queue");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        let event = serde_json::json!({
            "session_id": created.claude_session_id,
            "hook_event_name": "UserPromptSubmit",
            "prompt": "实现会话状态管理"
        });

        let response = queue_hook_event(&events_dir, &event).unwrap().unwrap();
        assert_eq!(
            response["hookSpecificOutput"]["sessionTitle"],
            "实现会话状态管理"
        );
        assert_eq!(manager.ingest_events().unwrap(), 1);
        assert_eq!(
            manager.get(&created.id).unwrap().status,
            SessionStatus::Running
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn direct_communication_foreign_instance_preserves_session_start_for_owner() {
        let (path, events) = test_paths("foreign-event-consumer");
        let foreign = SessionManager::open_at(path.clone(), events.clone()).unwrap();
        let owner = SessionManager::open_at(path.clone(), events.clone()).unwrap();
        let created = owner.create_runtime(NewRuntimeSession {
            runtime_kind: "claude_code".into(), cwd: ".".into(), project_id: None,
        }).unwrap();
        owner.attach_runtime(&created.id, 1).unwrap();
        queue_hook_event(&events, &serde_json::json!({
            "session_id": created.claude_session_id, APP_SESSION_ID_FIELD: created.id,
            "hook_event_name": "SessionStart"
        })).unwrap();
        assert_eq!(foreign.ingest_events().unwrap(), 0, "foreign desktop consumed readiness");
        assert_eq!(owner.ingest_events().unwrap(), 1);
        assert!(owner.is_ready_for_prompt(&created.id));
        drop(owner); drop(foreign);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn concurrent_event_ingest_processes_each_file_once() {
        let (path, events_dir) = test_paths("concurrent-event-ingest");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        queue_hook_event(
            &events_dir,
            &serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "UserPromptSubmit",
                "prompt": "并发摄取"
            }),
        )
        .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let handles = (0..2)
            .map(|_| {
                let manager = manager.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    manager.ingest_events()
                })
            })
            .collect::<Vec<_>>();
        barrier.wait();
        let results = handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect::<Vec<_>>();
        assert!(results.iter().all(Result::is_ok));
        assert_eq!(results.into_iter().map(Result::unwrap).sum::<usize>(), 1);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
    #[test]
    fn hook_command_carries_the_stable_app_session_id() {
        let (path, events_dir) = test_paths("hook-command-session-id");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();

        let command = manager.hook_command("app-session-123").unwrap();
        let status_line = manager.status_line_command("app-session-123").unwrap();

        assert!(command.contains("--session-hook"));
        assert!(command.contains("\"app-session-123\""));
        assert!(status_line.contains("--session-statusline"));
        assert!(status_line.contains("\"app-session-123\""));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn manual_title_is_preserved_across_waiting_and_completion_events() {
        let (path, events_dir) = test_paths("lifecycle");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.rename(&created.id, "登录模块修复").unwrap();

        for (event, expected) in [
            (
                serde_json::json!({
                    "session_id": created.claude_session_id,
                    "hook_event_name": "UserPromptSubmit",
                    "prompt": "这个标题不应该覆盖手动标题"
                }),
                SessionStatus::Running,
            ),
            (
                serde_json::json!({
                    "session_id": created.claude_session_id,
                    "hook_event_name": "Notification",
                    "notification_type": "permission_prompt"
                }),
                SessionStatus::Waiting,
            ),
            (
                serde_json::json!({
                    "session_id": created.claude_session_id,
                    "hook_event_name": "Notification",
                    "notification_type": "idle_prompt"
                }),
                SessionStatus::Completed,
            ),
        ] {
            manager.apply_hook_value(&event).unwrap();
            let session = manager.get(&created.id).unwrap();
            assert_eq!(session.status, expected);
            assert_eq!(session.title, "登录模块修复");
        }

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn submitted_terminal_input_resumes_a_session_after_permission_confirmation() {
        let (path, events_dir) = test_paths("permission-resume");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 41).unwrap();

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "UserPromptSubmit",
                "prompt": "执行需要权限的工具"
            }))
            .unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Notification",
                "notification_type": "permission_prompt"
            }))
            .unwrap();
        assert_eq!(
            manager.get(&created.id).unwrap().status,
            SessionStatus::Waiting
        );

        manager.mark_terminal_submission(41).unwrap();

        assert_eq!(
            manager.get(&created.id).unwrap().status,
            SessionStatus::Running
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn runtime_prompt_lifecycle_and_command_host_survive_reopen() {
        let (path, events_dir) = test_paths("runtime-messaging-lifecycle");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let session = manager
            .create_runtime(NewRuntimeSession {
                runtime_kind: "opencode".into(),
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&session.id, 13).unwrap();
        assert!(manager.observe_runtime_prompt_output(&session.id, "idle", |text| text == "idle"));
        assert!(manager.is_ready_for_prompt(&session.id));
        manager.observe_runtime_prompt_output(&session.id, "busy", |_| false);
        assert!(!manager.is_ready_for_prompt(&session.id));
        manager.mark_runtime_prompt_ready(&session.id);
        manager.mark_terminal_submission(13).unwrap();
        assert!(!manager.is_ready_for_prompt(&session.id));
        manager.set_runtime_command_host(&session.id, true).unwrap();
        assert!(!manager.mark_runtime_prompt_ready(&session.id));
        let reopened = SessionManager::open_at(path.clone(), events_dir).unwrap();
        assert!(reopened.get(&session.id).unwrap().runtime_command_host);
        assert!(!reopened.mark_runtime_prompt_ready(&session.id));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn bridge_resume_uses_only_verified_native_locator_and_never_falls_back_to_fresh() {
        for kind in ["pi"] {
            let (path, events) = test_paths("bridge-native-resume");
            let manager = SessionManager::open_at(path.clone(), events).unwrap();
            let record = manager
                .create_runtime(NewRuntimeSession {
                    runtime_kind: kind.into(),
                    cwd: ".".into(),
                    project_id: None,
                })
                .unwrap();
            let mut fresh = std::process::Command::new(kind);
            manager
                .configure_runtime_resume(&mut fresh, &record)
                .unwrap();
            assert_eq!(fresh.get_args().count(), 0);
            manager.attach_runtime(&record.id, 11).unwrap();
            manager
                .runtime_bridge_tokens
                .lock()
                .unwrap()
                .insert(record.id.clone(), ("launch".into(), 0));
            fs::create_dir_all(manager.runtime_locator_path(&record.id).parent().unwrap()).unwrap();
            let file = path.parent().unwrap().join("original-pi-session.jsonl");
            fs::write(&file, "history preserved").unwrap();
            let locator = file.display().to_string();
            let mut event = serde_json::json!({ "session_id": record.id, APP_SESSION_ID_FIELD: record.id,
                "hook_event_name": "RuntimePromptState", "runtime_kind": kind, "runtime_token": "stale",
                "runtime_sequence": 1, "ready": true, "runtime_session_locator": locator });
            assert!(manager.apply_hook_value(&event).unwrap().is_none());
            assert!(!manager.runtime_locator_path(&record.id).exists());
            event["runtime_token"] = "launch".into();
            manager.apply_hook_value(&event).unwrap().unwrap();
            event["runtime_sequence"] = 2.into();
            event["ready"] = false.into();
            event["message_error"] = "消息未确认，未自动重发".into();
            manager.apply_hook_value(&event).unwrap().unwrap();
            let inbox = manager.runtime_inbox_dir(&record.id);
            fs::create_dir_all(&inbox).unwrap();
            fs::write(
                inbox.join("late.accepted.json"),
                serde_json::to_vec(&serde_json::json!({
                    "sessionId": record.id, "messageId": "late", "status": "accepted",
                }))
                .unwrap(),
            )
            .unwrap();
            assert!(manager.runtime_message_accepted(&record.id, "late"));
            assert!(
                manager.get(&record.id).unwrap().last_error.is_none(),
                "late ACK clears only bridge-owned error"
            );
            event["runtime_sequence"] = 3.into();
            manager.apply_hook_value(&event).unwrap().unwrap();
            manager
                .data
                .lock()
                .unwrap()
                .sessions
                .iter_mut()
                .find(|session| session.id == record.id)
                .unwrap()
                .last_error = Some("无关模型错误".into());
            assert!(manager.runtime_message_accepted(&record.id, "late"));
            assert_eq!(
                manager.get(&record.id).unwrap().last_error.as_deref(),
                Some("无关模型错误")
            );
            manager.mark_stopped(&record.id).unwrap();
            let stopped = manager.get(&record.id).unwrap();
            let mut resumed = std::process::Command::new(kind);
            manager
                .configure_runtime_resume(&mut resumed, &stopped)
                .unwrap();
            assert_eq!(
                resumed
                    .get_args()
                    .map(|arg| arg.to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
                vec![
                    if kind == "pi" {
                        "--session".into()
                    } else {
                        "--resume".into()
                    },
                    locator
                ]
            );
            if kind == "pi" {
                fs::remove_file(&file).unwrap();
                assert!(manager
                    .configure_runtime_resume(&mut std::process::Command::new(kind), &stopped)
                    .is_err());
            }
            fs::remove_file(manager.runtime_locator_path(&record.id)).unwrap();
            assert!(manager
                .configure_runtime_resume(&mut std::process::Command::new(kind), &stopped)
                .is_err());
            fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    #[test]
    fn pi_first_launch_keeps_early_ready_event_until_pty_binding() {
        let (path, events) = test_paths("pi-first-launch");
        let manager = SessionManager::open_at(path.clone(), events.clone()).unwrap();
        let record = manager
            .create_agent(NewAgentSession {
                avatar_id: None,
                project_id: "project".into(),
                agent_id: "pi-agent".into(),
                runtime_kind: "pi".into(),
                agent_name: "Pi".into(),
                cwd: ".".into(),
            })
            .unwrap();
        manager.set_runtime_command_host(&record.id, true).unwrap();
        let starting = manager.begin_runtime_start(&record.id).unwrap();
        let mut command = std::process::Command::new("pi");
        let token = manager
            .configure_pi_readiness(&mut command, &record.id)
            .unwrap();
        let ready = serde_json::json!({"session_id": record.id, APP_SESSION_ID_FIELD: record.id,
            "hook_event_name": "RuntimePromptState", "runtime_kind": "pi", "runtime_token": token,
            "runtime_sequence": 1, "ready": true});
        let early = events.join("early.json");
        fs::write(&early, serde_json::to_vec(&ready).unwrap()).unwrap();
        // Force the exact interleaving: event + legacy migration before open() returns.
        assert_eq!(manager.ingest_events().unwrap(), 0);
        assert!(early.exists());
        assert!(!manager
            .migrate_inactive_runtime_command_host(&record.id, |_| false)
            .unwrap());
        assert!(!manager.get(&record.id).unwrap().runtime_command_host);
        assert!(!manager.is_ready_for_prompt(&record.id));
        manager.attach_runtime(&record.id, 42).unwrap();
        assert_eq!(manager.ingest_events().unwrap(), 0);
        drop(starting);
        assert!(!manager
            .migrate_inactive_runtime_command_host(&record.id, |id| id == 42)
            .unwrap());
        assert_eq!(manager.ingest_events().unwrap(), 1);
        assert!(!early.exists());
        assert!(manager.is_ready_for_prompt(&record.id));
        // A subsequent launch still rejects an old authenticated launch's event.
        let next = manager.begin_runtime_start(&record.id).unwrap();
        manager
            .configure_pi_readiness(&mut command, &record.id)
            .unwrap();
        fs::write(&early, serde_json::to_vec(&ready).unwrap()).unwrap();
        manager.attach_runtime(&record.id, 43).unwrap();
        drop(next);
        assert_eq!(manager.ingest_events().unwrap(), 1);
        assert!(!manager.is_ready_for_prompt(&record.id));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn runtime_start_failure_releases_migration_and_duplicate_start_fails() {
        let (path, events) = test_paths("runtime-start-failure");
        let manager = SessionManager::open_at(path.clone(), events).unwrap();
        let record = manager
            .create_runtime(NewRuntimeSession {
                runtime_kind: "pi".into(),
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        let launch = manager.begin_runtime_start(&record.id).unwrap();
        assert!(manager.begin_runtime_start(&record.id).is_err());
        assert!(!manager
            .migrate_inactive_runtime_command_host(&record.id, |_| false)
            .unwrap());
        drop(launch); // spawn error exits the scope through the same RAII path
        assert!(manager
            .migrate_inactive_runtime_command_host(&record.id, |_| false)
            .unwrap());
        assert!(manager.get(&record.id).unwrap().runtime_command_host);
        assert!(!manager.is_ready_for_prompt(&record.id));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn pi_native_completion_requires_authenticated_matching_task_and_keeps_inbox_ready() {
        let (path, events) = test_paths("pi-native-turn");
        let manager = SessionManager::open_at(path.clone(), events).unwrap();
        let record = manager
            .create_agent(NewAgentSession {
                avatar_id: None,
                project_id: "project".into(),
                agent_id: "pi-agent".into(),
                runtime_kind: "pi".into(),
                agent_name: "Pi".into(),
                cwd: ".".into(),
            })
            .unwrap();
        manager.attach_runtime(&record.id, 13).unwrap();
        manager.set_runtime_command_host(&record.id, false).unwrap();
        let mut command = std::process::Command::new("pi");
        let token = manager
            .configure_pi_readiness(&mut command, &record.id)
            .unwrap();
        manager.register_agent_prompt(&record.id, "build task");
        let mut event = serde_json::json!({"session_id": record.id, APP_SESSION_ID_FIELD: record.id,
            "hook_event_name": "RuntimePromptState", "runtime_kind": "pi", "runtime_token": "stale",
            "runtime_sequence": 1, "ready": true,
            "turn_completion": {"prompt": "build task", "summary": "done"}});
        assert!(manager.apply_hook_value(&event).unwrap().is_none());
        assert_eq!(manager.agent_turn_revision(&record.id), 0);
        event["runtime_token"] = token.into();
        event["turn_completion"]["prompt"] = "inbound chat".into();
        manager.apply_hook_value(&event).unwrap().unwrap();
        assert_eq!(manager.agent_turn_revision(&record.id), 0);
        event["runtime_sequence"] = 2.into();
        event["turn_completion"]["prompt"] = "build task".into();
        manager.apply_hook_value(&event).unwrap().unwrap();
        assert_eq!(
            manager.agent_turn_summary_after(&record.id, 0, "build task"),
            Some("done".into())
        );
        assert!(manager.is_ready_for_prompt(&record.id));
        assert!(!manager.get(&record.id).unwrap().runtime_command_host);
        assert!(manager
            .queue_runtime_delivery(&record.id, "next-message", "hello")
            .unwrap());
        let failure = manager
            .runtime_inbox_dir(&record.id)
            .join("task-unconfirmed.unconfirmed.json");
        fs::write(
            &failure,
            serde_json::to_vec(&serde_json::json!({"sessionId": record.id,
            "messageId": "task-unconfirmed", "error": "Pi did not accept input"}))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            manager.runtime_delivery_error(&record.id, "task-unconfirmed"),
            Some("Pi did not accept input".into())
        );
        assert!(manager
            .runtime_delivery_error(&record.id, "unrelated-message")
            .is_none());
        fs::write(manager.runtime_inbox_dir(&record.id).join("task-unconfirmed.accepted.json"),
            serde_json::to_vec(&serde_json::json!({"sessionId": record.id, "messageId": "task-unconfirmed", "status": "accepted"})).unwrap()).unwrap();
        assert!(
            manager
                .runtime_delivery_error(&record.id, "task-unconfirmed")
                .is_none(),
            "late receipt supersedes warning"
        );
        manager.register_agent_prompt(&record.id, "next task");
        event["runtime_sequence"] = 3.into();
        event["turn_completion"] = serde_json::json!({"prompt": "next task", "summary": "", "error": "model authentication failed"});
        manager.apply_hook_value(&event).unwrap().unwrap();
        assert_eq!(
            manager.agent_turn_summary_after(&record.id, 1, "next task"),
            Some("".into())
        );
        assert_eq!(
            manager.get(&record.id).unwrap().last_error.as_deref(),
            Some("model authentication failed")
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn zcode_prompt_tracks_visible_cells_across_dispatch_resize_and_out_of_order_redraws() {
        let (path, events_dir) = test_paths("zcode-visible-composer");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let session = manager.create_runtime(NewRuntimeSession { runtime_kind: "zcode_cli".into(), cwd: ".".into(), project_id: None }).unwrap();
        manager.attach_runtime(&session.id, 14).unwrap();
        manager.resize_runtime_prompt(&session.id, 12, 80);
        let observe = |raw: &str| manager.observe_runtime_prompt_output_with_blocker(&session.id, raw,
            |text| crate::multi_agent::interactive_runtime_prompt_ready(crate::multi_agent::AgentRuntimeKind::ZCodeCli, text),
            crate::multi_agent::interactive_runtime_prompt_blocked);
        assert!(observe("\x1b[2J\x1b[10;1HType a prompt"));
        manager.mark_prompt_dispatched(&session.id);
        assert!(!observe("\x1b[1;1HZCODE"), "an unchanged idle composer cannot authorize another paste");
        assert!(!observe("\x1b[10;1H\x1b[2K/help"));
        manager.mark_prompt_dispatched(&session.id);
        assert!(observe("\x1b[10;1H\x1b[2KType a prompt"));
        // OpenTUI can redraw history AFTER the lower input pane. Byte order
        // differs from the screen's reading order; help is not a live dialog.
        observe("\x1b[2;1HSlash command help: permission required, enter to confirm");
        assert!(manager.is_ready_for_prompt(&session.id));
        manager.resize_runtime_prompt(&session.id, 14, 60);
        observe("\x1b[1;1HZCODE");
        assert!(manager.is_ready_for_prompt(&session.id));
        assert!(!observe("\x1b[10;1H\x1b[2KType to queue input\x1b[11;1Hesc to interrupt"));
        assert!(!observe("\x1b[2J\x1b[HPermission required: Allow once"));
        assert!(!observe("\x1b[?25h"));
        assert!(!observe("\x1b[2J\x1b[10;1HType a "));
        assert!(observe("prompt"));
        // Overwriting the placeholder while typing must revoke readiness.
        assert!(!observe("\x1b[10;1H\x1b[2Kuser draft"));
        assert!(observe("\x1b[10;1H\x1b[2KType a prompt"));
        manager.mark_stopped(&session.id).unwrap();
        assert!(!manager.runtime_prompt_buffers.lock().unwrap().contains_key(&session.id));
        drop(manager);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn busy_prompt_fragments_do_not_poison_the_next_idle_redraw() {
        let (path, events_dir) = test_paths("runtime-message-redraw");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let session = manager
            .create_runtime(NewRuntimeSession {
                runtime_kind: "kimi_code".into(),
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&session.id, 14).unwrap();
        let observe = |data: &str| {
            manager.observe_runtime_prompt_output_with_blocker(
                &session.id,
                data,
                |text| {
                    crate::multi_agent::interactive_runtime_prompt_ready(
                        crate::multi_agent::AgentRuntimeKind::KimiCode,
                        text,
                    )
                },
                crate::multi_agent::interactive_runtime_prompt_blocked,
            )
        };
        assert!(!observe("Working (esc to "));
        assert!(!observe("cancel)"));
        assert!(!manager.is_ready_for_prompt(&session.id));
        assert!(!observe("\x1b[2J\r\n│ "));
        assert!(observe("> \x1b[7m \x1b[27m│\r\nK3 thinking: high"));
        assert!(manager.is_ready_for_prompt(&session.id));
        manager.mark_prompt_dispatched(&session.id);
        assert!(observe(
            "Working (esc to cancel)\x1b[2J\x1b[H│ >  │\r\nK3 thinking: high"
        ));
        assert!(manager.is_ready_for_prompt(&session.id));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn codex_ready_frame_survives_cosmetic_and_fragmented_footer_redraws() {
        let (path, events_dir) = test_paths("codex-ready-cosmetic-tail");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let record = manager.create_runtime(NewRuntimeSession {
            runtime_kind: "codex".into(), cwd: ".".into(), project_id: None,
        }).unwrap();
        manager.attach_runtime(&record.id, 51).unwrap();
        let observe = |output: &str| manager.observe_runtime_prompt_output_with_blocker(
            &record.id, output,
            |text| crate::multi_agent::interactive_runtime_prompt_ready(
                crate::multi_agent::AgentRuntimeKind::Codex, text,
            ),
            crate::multi_agent::interactive_runtime_prompt_blocked,
        );
        assert!(observe("Worked for 10s · 14:41\n› Ask Codex to do anything\n"));
        for fragment in ["\x1b[?25h", "\x1b[0m", "\r\n \t", "  GPT-6.1-", "Sol medium", " · C:\\fixture", "\r\n"] {
            observe(fragment);
            assert!(manager.is_ready_for_prompt(&record.id), "cosmetic/footer fragment revoked an established composer: {fragment:?}");
            assert!(!manager.runtime_prompt_is_blocked(&record.id));
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn codex_update_skip_releases_on_fresh_native_idle_without_retyping() {
        let (path, events_dir) = test_paths("codex-update-idle");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let record = manager.create_runtime(NewRuntimeSession {
            runtime_kind: "codex".into(), cwd: ".".into(), project_id: None,
        }).unwrap();
        manager.attach_runtime(&record.id, 61).unwrap();
        let observe = |output: &str| manager.observe_runtime_prompt_output_with_blocker(
            &record.id, output,
            |text| crate::multi_agent::interactive_runtime_prompt_ready(crate::multi_agent::AgentRuntimeKind::Codex, text),
            crate::multi_agent::interactive_runtime_prompt_blocked,
        );
        // The 0.167 TUI paints incrementally, and need not redraw its composer
        // after initialization. An idle marker before the dialog cannot open it.
        assert!(!observe("\x1b]7501;state=idle:app=codex\x1b\\\nUpdat available\n1. Update now\n2.Skip\nenter continue · esc skip"));
        assert!(manager.runtime_prompt_is_blocked(&record.id));
        assert!(!observe("\x1b]7501;state=working:app=codex\x1b\\"));
        assert!(observe("\x1b]7501;state=idle:app=codex\x1b\\"), "fresh post-update idle must release the queued prompt without manual Enter");
        assert!(manager.is_ready_for_prompt(&record.id));
        // Login/approval dialogs remain protected even if native idle arrives.
        assert!(!observe("Permission required\nAllow once\nenter to confirm"));
        assert!(!observe("\x1b]7501;state=idle:app=codex\x1b\\"));
        assert!(!manager.is_ready_for_prompt(&record.id));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn codex_ready_frame_is_revoked_by_busy_dialogs_and_unknown_new_screens() {
        let (path, events_dir) = test_paths("codex-ready-new-state");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let record = manager.create_runtime(NewRuntimeSession {
            runtime_kind: "codex".into(), cwd: ".".into(), project_id: None,
        }).unwrap();
        manager.attach_runtime(&record.id, 52).unwrap();
        let observe = |output: &str| manager.observe_runtime_prompt_output_with_blocker(
            &record.id, output,
            |text| crate::multi_agent::interactive_runtime_prompt_ready(
                crate::multi_agent::AgentRuntimeKind::Codex, text,
            ),
            crate::multi_agent::interactive_runtime_prompt_blocked,
        );
        let composer = "\x1b[2J\x1b[HWorked for 10s · 14:41\n› Ask Codex to do anything\n";
        for blocker in [
            "Working (10s • esc to interrupt)\n› Ask Codex to do anything\nGPT-6.1-Sol medium",
            "Permission required\nAllow once\nenter to confirm",
            "Update available!\n1. Update now\n2. Skip\nenter continue · esc skip",
        ] {
            assert!(observe(composer));
            assert!(!observe(blocker));
            assert!(manager.runtime_prompt_is_blocked(&record.id), "new state did not block the retained composer: {blocker:?}");
            assert!(!manager.is_ready_for_prompt(&record.id));
            assert!(!observe("\x1b[?25h\x1b[0m\r\n"));
            assert!(!manager.is_ready_for_prompt(&record.id), "cosmetic output cannot dismiss a blocker");
        }
        assert!(observe(composer));
        assert!(!observe("\x1b[2J\x1b[HLoading"));
        assert!(!manager.is_ready_for_prompt(&record.id), "a cleared screen must not inherit the old composer");
        assert!(!observe("\x1b[?25h\x1b[0m\r\n"));
        assert!(!manager.is_ready_for_prompt(&record.id));
        assert!(observe(composer));
        assert!(manager.is_ready_for_prompt(&record.id));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn codex_dispatch_discards_the_ready_frame_before_cosmetic_redraws() {
        let (path, events_dir) = test_paths("codex-dispatched-cosmetic-tail");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let record = manager.create_runtime(NewRuntimeSession {
            runtime_kind: "codex".into(), cwd: ".".into(), project_id: None,
        }).unwrap();
        manager.attach_runtime(&record.id, 53).unwrap();
        let observe = |output: &str| manager.observe_runtime_prompt_output_with_blocker(
            &record.id, output,
            |text| crate::multi_agent::interactive_runtime_prompt_ready(
                crate::multi_agent::AgentRuntimeKind::Codex, text,
            ),
            crate::multi_agent::interactive_runtime_prompt_blocked,
        );
        let composer = "Worked for 10s · 14:41\n› Ask Codex to do anything\n";
        assert!(observe(composer));
        manager.mark_prompt_dispatched(&record.id);
        assert!(!manager.is_ready_for_prompt(&record.id));
        for fragment in ["\x1b[?25h", "\x1b[0m", "\r\n \t", "GPT-6.1-Sol medium · C:\\fixture\r\n"] {
            assert!(!observe(fragment));
            assert!(!manager.is_ready_for_prompt(&record.id), "old composer revived after dispatch: {fragment:?}");
        }
        assert!(observe(composer));
        assert!(manager.is_ready_for_prompt(&record.id));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn runtime_prompt_blocker_survives_dispatch_and_partial_redraw_until_fresh_ready() {
        let (path, events_dir) = test_paths("runtime-blocker-state");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let record = manager.create_runtime(NewRuntimeSession {
            runtime_kind: "codex".into(), cwd: ".".into(), project_id: None,
        }).unwrap();
        manager.attach_runtime(&record.id, 41).unwrap();
        assert!(manager.mark_runtime_prompt_ready(&record.id));
        assert!(!manager.observe_runtime_prompt_output_with_blocker(
            &record.id, "update dialog", |_| true, |_| true,
        ));
        assert!(manager.runtime_prompt_is_blocked(&record.id));
        assert!(!manager.is_ready_for_prompt(&record.id));
        manager.mark_prompt_dispatched(&record.id);
        assert!(manager.runtime_prompt_is_blocked(&record.id), "dispatch must not dismiss a dialog");
        assert!(!manager.observe_runtime_prompt_output_with_blocker(
            &record.id, "partial redraw", |_| false, |_| false,
        ));
        assert!(manager.runtime_prompt_is_blocked(&record.id));
        // A hook/fallback reporting turn completion is not evidence that the
        // independent TUI dialog has gone away.
        manager.ready_for_prompt.lock().unwrap().insert(record.id.clone());
        assert!(!manager.is_ready_for_prompt(&record.id));
        manager.observe_runtime_prompt_output_with_blocker(
            &record.id, "fresh input prompt", |_| true, |_| false,
        );
        assert!(!manager.runtime_prompt_is_blocked(&record.id));
        assert!(manager.is_ready_for_prompt(&record.id));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn runtime_prompt_blocker_is_scoped_to_the_bound_process_and_cleared_on_stop() {
        let (path, events_dir) = test_paths("runtime-blocker-lifecycle");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let record = manager.create_runtime(NewRuntimeSession {
            runtime_kind: "codex".into(), cwd: ".".into(), project_id: None,
        }).unwrap();
        let block = || {
            manager.observe_runtime_prompt_output_with_blocker(&record.id, "dialog", |_| false, |_| true);
            assert!(manager.runtime_prompt_is_blocked(&record.id));
        };
        manager.attach_runtime(&record.id, 41).unwrap();
        block();
        manager.attach_runtime(&record.id, 42).unwrap();
        assert!(!manager.runtime_prompt_is_blocked(&record.id), "new process must not inherit the old dialog");
        block();
        assert!(manager.mark_exit_if_pty(&record.id, 41, 0).unwrap().is_none());
        assert!(manager.runtime_prompt_is_blocked(&record.id), "a stale process exit must not clear the current dialog");
        manager.mark_stopped(&record.id).unwrap();
        assert!(!manager.runtime_prompt_is_blocked(&record.id));
        manager.attach_runtime(&record.id, 43).unwrap();
        block();
        manager.mark_exit_if_pty(&record.id, 43, 0).unwrap().unwrap();
        assert!(!manager.runtime_prompt_is_blocked(&record.id));
        manager.attach_runtime(&record.id, 44).unwrap();
        block();
        manager.mark_error(&record.id, "fixture failure").unwrap();
        assert!(!manager.runtime_prompt_is_blocked(&record.id));
        manager.attach_runtime(&record.id, 45).unwrap();
        block();
        manager.mark_all_stopped().unwrap();
        assert!(!manager.runtime_prompt_is_blocked(&record.id));
        manager.attach_runtime(&record.id, 46).unwrap();
        block();
        manager.delete(&record.id).unwrap();
        assert!(!manager.runtime_prompt_is_blocked(&record.id));
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn post_tool_use_recovers_a_stale_waiting_session() {
        let (path, events_dir) = test_paths("permission-post-tool-recovery");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Notification",
                "notification_type": "permission_prompt"
            }))
            .unwrap();

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "PostToolUse",
                "tool_name": "Bash"
            }))
            .unwrap();

        assert_eq!(
            manager.get(&created.id).unwrap().status,
            SessionStatus::Running
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn session_end_keeps_attached_runtime_active_until_pty_exits() {
        let (path, events_dir) = test_paths("session-end-runtime");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 17).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "UserPromptSubmit",
                "prompt": "执行一个长任务"
            }))
            .unwrap();
        let renamed = manager.rename(&created.id, "执行中的会话").unwrap();
        assert_eq!(renamed.status, SessionStatus::Running);

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionEnd",
                "reason": "clear"
            }))
            .unwrap();

        let active = manager.get(&created.id).unwrap();
        assert_eq!(active.status, SessionStatus::Running);
        assert_eq!(active.pty_id, Some(17));
        assert_eq!(active.title, "执行中的会话");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn clear_rebinds_new_claude_session_to_same_app_session() {
        let (path, events_dir) = test_paths("clear-rebind");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: true,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 18).unwrap();
        manager.rename(&created.id, "手动标题").unwrap();

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionEnd",
                "reason": "clear",
                "_crewvela_app_session_id": created.id
            }))
            .unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": "new-claude-session-id",
                "hook_event_name": "SessionStart",
                "source": "clear",
                "_crewvela_app_session_id": created.id
            }))
            .unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionEnd",
                "reason": "clear",
                "_crewvela_app_session_id": created.id
            }))
            .unwrap();

        let rebound = manager.get(&created.id).unwrap();
        assert_eq!(rebound.claude_session_id, "new-claude-session-id");
        assert_eq!(rebound.status, SessionStatus::Pending);
        assert_eq!(rebound.pty_id, Some(18));
        assert_eq!(rebound.title, "手动标题");

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": "new-claude-session-id",
                "hook_event_name": "UserPromptSubmit",
                "prompt": "继续执行",
                "_crewvela_app_session_id": created.id
            }))
            .unwrap();
        let running = manager.get(&created.id).unwrap();
        assert_eq!(running.status, SessionStatus::Running);
        assert_eq!(running.title, "手动标题");

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn pasted_transcript_unwraps_only_a_complete_matched_outer_tag() {
        for text in ["<pasted_content id=\"08a7\">\nhello\n</pasted_content id=\"08a7\">", "<pasted_content id=\"08a7\">hello</pasted_content>"] {
            assert_eq!(claude_pasted_transcript_text(text), "hello");
        }
        for text in ["prefix <pasted_content id=\"08a7\">hello</pasted_content>", "<pasted_content id=\"08a7\">hello</pasted_content id=\"other\">", "<pasted_content id=\"08a7\">hello", "<pasted_content id=\"08a7\">hello</pasted_content> suffix"] {
            assert_eq!(claude_pasted_transcript_text(text), text);
        }
    }

    #[test]
    fn claude_pasted_transcript_stop_releases_next_message_and_rejects_stale_turn() {
        let (path, events) = test_paths("claude-pasted-stop");
        let manager = SessionManager::open_at(path.clone(), events).unwrap();
        let session = manager.create_agent(NewAgentSession {
            avatar_id: None, project_id: "project".into(), agent_id: "agent".into(),
            runtime_kind: "claude_code".into(), agent_name: "Claude".into(), cwd: path.parent().unwrap().display().to_string(),
        }).unwrap();
        manager.attach_runtime(&session.id, 19).unwrap();
        let prompt = "[CrewVela 多 Agent 协作消息]\n仅回复 READY";
        let transcript = path.parent().unwrap().join("native-claude.jsonl");
        let pasted_prompt = format!("\n\n<pasted_content id=\"08a7\">\n{prompt}\n</pasted_content id=\"08a7\">\n\n");
        fs::write(&transcript, serde_json::json!({"type":"user","message":{"content":pasted_prompt}}).to_string()).unwrap();
        manager.apply_hook_value(&serde_json::json!({APP_SESSION_ID_FIELD:session.id,"session_id":"native-claude","hook_event_name":"SessionStart"})).unwrap().expect("bind native identity before the turn");
        let submit = |prompt: &str| serde_json::json!({APP_SESSION_ID_FIELD:session.id,"session_id":"native-claude", "hook_event_name":"UserPromptSubmit","prompt":prompt});
        manager.apply_hook_value(&submit(&pasted_prompt)).unwrap().expect("real submit must be accepted");
        let mut stop = serde_json::json!({APP_SESSION_ID_FIELD:session.id,"session_id":"native-claude", "hook_event_name":"Stop","transcript_path":transcript,"last_assistant_message":"READY"});
        enrich_stop_event_summary(&mut stop);
        manager.apply_hook_value(&stop).unwrap();
        assert!(manager.is_ready_for_prompt(&session.id), "native paste wrapper must not strand the next queued message");
        assert_eq!(manager.agent_turn_revision(&session.id), 1);
        manager.apply_hook_value(&submit("newer turn")).unwrap();
        manager.apply_hook_value(&stop).unwrap();
        assert!(!manager.is_ready_for_prompt(&session.id), "an old stop must not complete the newer turn");
        assert_eq!(manager.agent_turn_revision(&session.id), 1);
        drop(manager);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn stop_with_background_tasks_stays_running_until_every_task_finishes() {
        let (path, events_dir) = test_paths("background-task-status");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: true,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 19).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "UserPromptSubmit",
                "prompt": "启动后台构建"
            }))
            .unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Notification",
                "notification_type": "idle_prompt"
            }))
            .unwrap();
        assert_eq!(
            manager.get(&created.id).unwrap().status,
            SessionStatus::Running
        );

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Stop",
                "background_tasks": [{
                    "id": "task-1",
                    "type": "shell",
                    "status": "running",
                    "description": "构建应用"
                }],
                "session_crons": []
            }))
            .unwrap();
        assert_eq!(
            manager.get(&created.id).unwrap().status,
            SessionStatus::Running
        );

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Notification",
                "notification_type": "idle_prompt"
            }))
            .unwrap();
        assert_eq!(
            manager.get(&created.id).unwrap().status,
            SessionStatus::Running
        );

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Stop",
                "background_tasks": [],
                "session_crons": []
            }))
            .unwrap();
        assert_eq!(
            manager.get(&created.id).unwrap().status,
            SessionStatus::Completed
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn stop_after_clear_uses_the_transcript_session_id() {
        let (path, events_dir) = test_paths("clear-stop-transcript-id");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: true,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 20).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": "new-claude-session-id",
                "transcript_path": "C:/Users/demo/.claude/projects/demo/new-claude-session-id.jsonl",
                "hook_event_name": "SessionStart",
                "source": "clear",
                "_crewvela_app_session_id": created.id
            }))
            .unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": "new-claude-session-id",
                "transcript_path": "C:/Users/demo/.claude/projects/demo/new-claude-session-id.jsonl",
                "hook_event_name": "UserPromptSubmit",
                "prompt": "继续处理",
                "_crewvela_app_session_id": created.id
            }))
            .unwrap();

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.id,
                "transcript_path": "C:/Users/demo/.claude/projects/demo/new-claude-session-id.jsonl",
                "hook_event_name": "Stop",
                "background_tasks": [],
                "session_crons": [],
                "_crewvela_app_session_id": created.id
            }))
            .unwrap();

        let completed = manager.get(&created.id).unwrap();
        assert_eq!(completed.claude_session_id, "new-claude-session-id");
        assert_eq!(completed.status, SessionStatus::Completed);
        assert_eq!(completed.pty_id, Some(20));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn runtime_attachment_is_cleared_when_store_reopens() {
        let (path, events_dir) = test_paths("runtime");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: true,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 42).unwrap();
        assert_eq!(manager.get(&created.id).unwrap().pty_id, Some(42));
        drop(manager);

        let reopened = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let recovered = reopened.get(&created.id).unwrap();
        assert_eq!(recovered.pty_id, None);
        assert_eq!(recovered.status, SessionStatus::Stopped);

        reopened.delete(&created.id).unwrap();
        assert!(reopened.get(&created.id).is_none());

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn process_exit_detaches_runtime_without_claiming_task_completion() {
        let (path, events_dir) = test_paths("exit");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let completed = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&completed.id, 7).unwrap();
        manager.attach_runtime(&completed.id, 71).unwrap();
        assert!(manager.mark_exit_if_pty(&completed.id, 7, 1).unwrap().is_none());
        assert_eq!(manager.get(&completed.id).unwrap().pty_id, Some(71));
        manager.attach_runtime(&completed.id, 7).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": completed.claude_session_id,
                "hook_event_name": "Stop"
            }))
            .unwrap();

        let exited = manager.mark_exit(&completed.id, 0).unwrap().unwrap();
        assert_eq!(exited.status, SessionStatus::Completed);
        assert_eq!(exited.pty_id, None);

        let running = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&running.id, 8).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": running.claude_session_id,
                "hook_event_name": "UserPromptSubmit",
                "prompt": "运行任务"
            }))
            .unwrap();

        let exited = manager.mark_exit(&running.id, 0).unwrap().unwrap();
        assert_eq!(exited.status, SessionStatus::Stopped);
        assert_eq!(exited.pty_id, None);

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn queued_hook_events_are_ingested_in_capture_order() {
        let (path, events_dir) = test_paths("ordered-events");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        let started = serde_json::json!({
            "session_id": created.claude_session_id,
            "hook_event_name": "UserPromptSubmit",
            "prompt": "按顺序处理",
            "_crewvela_queued_at": 1
        });
        let stopped = serde_json::json!({
            "session_id": created.claude_session_id,
            "hook_event_name": "Stop",
            "_crewvela_queued_at": 2
        });
        fs::write(
            events_dir.join("z-first.json"),
            serde_json::to_vec(&started).unwrap(),
        )
        .unwrap();
        fs::write(
            events_dir.join("a-second.json"),
            serde_json::to_vec(&stopped).unwrap(),
        )
        .unwrap();

        assert_eq!(manager.ingest_events().unwrap(), 2);
        assert_eq!(
            manager.get(&created.id).unwrap().status,
            SessionStatus::Completed
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn readiness_follows_claude_lifecycle_for_broadcast_queue() {
        let (path, events_dir) = test_paths("broadcast-readiness");
        let manager = SessionManager::open_at(path.clone(), events_dir).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: "F:\\project\\demo".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();

        assert!(!manager.is_ready_for_prompt(&created.id));
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart"
            }))
            .unwrap();
        assert!(manager.is_ready_for_prompt(&created.id));

        manager.mark_prompt_dispatched(&created.id);
        assert!(!manager.is_ready_for_prompt(&created.id));

        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Stop"
            }))
            .unwrap();
        assert!(manager.is_ready_for_prompt(&created.id));

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn codex_turn_notification_completes_visible_agent_session() {
        let (path, events_dir) = test_paths("codex-agent-turn");
        let manager = SessionManager::open_at(path.clone(), events_dir.clone()).unwrap();
        let codex_home = path.parent().unwrap().join("codex-home");
        let created = manager
            .create_agent(NewAgentSession {
                avatar_id: None,
                project_id: "project-1".into(),
                agent_id: "codex-1".into(),
                runtime_kind: "codex".into(),
                agent_name: "后端".into(),
                cwd: path.parent().unwrap().display().to_string(),
            })
            .unwrap();
        manager.attach_runtime(&created.id, 31).unwrap();
        manager.register_agent_prompt(&created.id, "实现后端接口");
        write_codex_test_thread(&codex_home, "thread-123", "cli", &created.cwd);
        let payload = serde_json::json!({
            "type": "agent-turn-complete",
            "thread-id": "thread-123",
            "input-messages": ["实现后端接口"],
            "last-assistant-message": "后端接口已完成"
        })
        .to_string();

        run_agent_turn_hook(&events_dir, &created.id, &payload).unwrap();
        let queued = fs::read_dir(&events_dir).unwrap().map(|entry| entry.unwrap().path()).collect::<Vec<_>>();
        assert_eq!(queued.len(), 1);
        let event = serde_json::from_slice(&fs::read(&queued[0]).unwrap()).unwrap();
        assert!(manager.apply_hook_value_with_codex_home(&event, Some(&codex_home)).unwrap().is_some());
        let completed = manager.get(&created.id).unwrap();
        assert_eq!(completed.status, SessionStatus::Completed);
        assert_eq!(completed.claude_session_id, "thread-123");
        assert!(manager.is_ready_for_prompt(&created.id));
        assert_eq!(manager.agent_turn_revision(&created.id), 1);
        assert!(manager
            .agent_turn_summary_after(&created.id, 0, "上一轮任务")
            .is_none());
        assert_eq!(
            manager
                .agent_turn_summary_after(&created.id, 0, "实现后端接口")
                .as_deref(),
            Some("后端接口已完成")
        );

        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    fn write_codex_test_thread(home: &std::path::Path, id: &str, source: &str, cwd: &str) {
        let directory = home.join("sessions/2026/11/06");
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join(format!("rollout-fixture-{id}.jsonl")), serde_json::json!({
            "type": "session_meta", "payload": {"id": id, "source": source, "cwd": cwd}
        }).to_string()).unwrap();
    }

    #[test]
    fn codex_auxiliary_notifications_cannot_rebind_or_complete_the_main_thread() {
        let (path, events) = test_paths("codex-notify-identity");
        let manager = SessionManager::open_at(path.clone(), events).unwrap();
        let root = path.parent().unwrap();
        let codex_home = root.join("codex-home");
        let session = manager.create_agent(NewAgentSession {
            avatar_id: None, project_id: "project".into(), agent_id: "agent".into(),
            runtime_kind: "codex".into(), agent_name: "Codex".into(), cwd: root.display().to_string(),
        }).unwrap();
        manager.attach_runtime(&session.id, 41).unwrap();
        let event = |id: &str, prompt: &str, summary: &str| serde_json::json!({
            APP_SESSION_ID_FIELD: session.id, "session_id": id,
            "hook_event_name": "AgentTurnComplete", "input_prompt": prompt,
            "last_assistant_message": summary,
        });
        // Even before the main turn completes, ephemeral/title/subagent
        // notifications must not claim the initially empty native identity.
        assert!(manager.apply_hook_value_with_codex_home(&event("title", "title generation", "{\"title\":\"test\"}"), Some(&codex_home)).unwrap().is_none());
        write_codex_test_thread(&codex_home, "subagent", "exec", &session.cwd);
        assert!(manager.apply_hook_value_with_codex_home(&event("subagent", "task", "auxiliary"), Some(&codex_home)).unwrap().is_none());
        write_codex_test_thread(&codex_home, "wrong-metadata-id", "cli", &session.cwd);
        fs::rename(
            codex_home.join("sessions/2026/11/06/rollout-fixture-wrong-metadata-id.jsonl"),
            codex_home.join("sessions/2026/11/06/rollout-fixture-spoofed-id.jsonl"),
        ).unwrap();
        assert!(manager.apply_hook_value_with_codex_home(&event("spoofed-id", "task", "wrong metadata"), Some(&codex_home)).unwrap().is_none());
        let other_cwd = root.join("other-project");
        fs::create_dir(&other_cwd).unwrap();
        write_codex_test_thread(&codex_home, "other", "cli", &other_cwd.display().to_string());
        assert!(manager.apply_hook_value_with_codex_home(&event("other", "task", "other project"), Some(&codex_home)).unwrap().is_none());
        assert!(manager.get(&session.id).unwrap().claude_session_id.is_empty());
        assert!(!manager.runtime_locator_path(&session.id).exists());
        assert_eq!(manager.agent_turn_revision(&session.id), 0);

        write_codex_test_thread(&codex_home, "main", "cli", &session.cwd);
        manager.register_agent_prompt(&session.id, "main task");
        assert!(manager.apply_hook_value_with_codex_home(&event("main", "main task", "main result"), Some(&codex_home)).unwrap().is_some());
        let locator = fs::read(manager.runtime_locator_path(&session.id)).unwrap();
        assert!(manager.apply_hook_value_with_codex_home(&event("title", "title generation", "{\"title\":\"test\"}"), Some(&codex_home)).unwrap().is_none());
        assert_eq!(manager.get(&session.id).unwrap().claude_session_id, "main");
        assert_eq!(manager.agent_turn_revision(&session.id), 1);
        assert_eq!(fs::read(manager.runtime_locator_path(&session.id)).unwrap(), locator);
        assert_eq!(manager.agent_turn_summary_after(&session.id, 0, "main task").as_deref(), Some("main result"));

        // /new can legitimately change thread within the same PTY, but a
        // completion for another/missing input cannot update any identity.
        write_codex_test_thread(&codex_home, "new-main", "cli", &session.cwd);
        manager.register_agent_prompt(&session.id, "new task");
        for prompt in ["wrong task", ""] {
            assert!(manager.apply_hook_value_with_codex_home(&event("new-main", prompt, "wrong result"), Some(&codex_home)).unwrap().is_none());
            assert_eq!(manager.get(&session.id).unwrap().claude_session_id, "main");
            assert_eq!(fs::read(manager.runtime_locator_path(&session.id)).unwrap(), locator);
            assert_eq!(manager.agent_turn_revision(&session.id), 1);
        }
        assert!(manager.apply_hook_value_with_codex_home(&event("new-main", "new task", "new result"), Some(&codex_home)).unwrap().is_some());
        assert_eq!(manager.get(&session.id).unwrap().claude_session_id, "new-main");
        assert_eq!(manager.get(&session.id).unwrap().pty_id, Some(41));

        // Read-only CLI turns use their exact per-member scratch cwd, which
        // intentionally differs from the business cwd stored in the session.
        let scratch = root.join("multi-agent-readonly/project/agent");
        fs::create_dir_all(&scratch).unwrap();
        write_codex_test_thread(&codex_home, "readonly-main", "cli", &scratch.display().to_string());
        manager.attach_runtime(&session.id, 42).unwrap();
        manager.register_agent_prompt(&session.id, "readonly task");
        assert!(manager.apply_hook_value_with_codex_home(&event("readonly-main", "readonly task", "readonly result"), Some(&codex_home)).unwrap().is_some());
        assert_eq!(manager.get(&session.id).unwrap().claude_session_id, "readonly-main");
        assert_eq!(manager.get(&session.id).unwrap().pty_id, Some(42));
        assert_eq!(manager.agent_turn_revision(&session.id), 3);
        drop(manager);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn claude_stop_summary_is_read_from_native_transcript() {
        let root =
            std::env::temp_dir().join(format!("crewvela-transcript-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let transcript = root.join("session.jsonl");
        fs::write(
            &transcript,
            concat!(
                "{\"type\":\"user\",\"message\":{\"content\":\"开始\"}}\n",
                "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"前端页面与测试已完成\"}]}}\n",
                "{\"type\":\"user\",\"isMeta\":true,\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Skill 注入内容\"}]}}\n"
            ),
        )
        .unwrap();
        assert_eq!(
            last_message_from_transcript(&transcript, "assistant").as_deref(),
            Some("前端页面与测试已完成")
        );
        assert_eq!(
            last_message_from_transcript(&transcript, "user").as_deref(),
            Some("开始")
        );
        let _ = fs::remove_dir_all(root);
    }
}
