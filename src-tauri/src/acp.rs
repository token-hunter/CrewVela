use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::{Duration, Instant};

#[cfg(windows)]
#[path = "acp_job.rs"]
mod process_job;
#[cfg(windows)]
pub(crate) use process_job::ProcessJob;

#[cfg(any(unix, test))]
#[path = "acp_process_group_state.rs"]
mod process_group_state;

#[cfg(unix)]
#[path = "acp_process_group.rs"]
mod process_group;

const ACP_ADAPTER_PACKAGE: &str = "@agentclientprotocol/claude-agent-acp@0.63.0";
const CODEX_ACP_ADAPTER_PACKAGE: &str = "@agentclientprotocol/codex-acp@1.11.0";
const CODEX_ACP_NPX_BOOTSTRAP: &str = "node --eval \"import(require('node:url').pathToFileURL(process.env.CREWVELA_CODEX_ACP_WRAPPER).href)\"";
const START_TIMEOUT: Duration = Duration::from_secs(45);
const PROMPT_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ManagedAcpKind {
    ClaudeCode,
    Codex,
    DeepSeekHarness,
    Kimi,
    OpenCode,
}

impl ManagedAcpKind {
    fn label(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::Codex => "Codex",
            Self::DeepSeekHarness => "DeepSeek Harness",
            Self::Kimi => "Kimi Code",
            Self::OpenCode => "OpenCode",
        }
    }

    fn binary(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-agent-acp",
            Self::Codex => "codex-acp",
            Self::DeepSeekHarness => "dsh",
            Self::Kimi => "kimi",
            Self::OpenCode => "opencode",
        }
    }

    fn package(self) -> &'static str {
        match self {
            Self::ClaudeCode => ACP_ADAPTER_PACKAGE,
            Self::Codex => CODEX_ACP_ADAPTER_PACKAGE,
            Self::DeepSeekHarness => "@deepseek-ai/dsh",
            Self::Kimi => "@moonshot-ai/kimi-code",
            Self::OpenCode => "opencode-ai",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ManagedAcpMode {
    Inherit,
    ReadOnly,
    Default,
    Auto,
    FullAccess,
}

impl ManagedAcpMode {
    fn candidates(self) -> &'static [&'static str] {
        match self {
            Self::Inherit => &[],
            Self::ReadOnly => &["read-only", "plan"],
            Self::Default => &["agent", "default"],
            Self::Auto => &[
                "agent-full-access",
                "bypassPermissions",
                "acceptEdits",
                "agent",
            ],
            Self::FullAccess => &["agent-full-access", "bypassPermissions"],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ManagedAcpOutcome {
    Completed(ManagedAcpResult),
    Unavailable(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedAcpResult {
    pub text: String,
    pub session_id: String,
    pub agent_name: String,
    pub agent_version: String,
}

/// A conversation uses only the host's authenticated research gateway. Native tools,
/// user MCPs and executable configuration are not part of this permission ceiling.
#[derive(Clone, Copy)]
pub(crate) struct ManagedResearchPolicy {
    pub tools_enabled: bool,
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AcpCommand {
    pub name: String,
    pub description: String,
    pub input_hint: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpStatus {
    pub connected: bool,
    pub session_id: String,
    pub cwd: String,
    pub agent_name: String,
    pub agent_version: String,
    pub full_permissions: bool,
    pub resumed: bool,
    pub commands: Vec<AcpCommand>,
    pub config_options: Vec<Value>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AcpPromptResult {
    pub text: String,
    pub commands: Vec<AcpCommand>,
    pub cancelled: bool,
}

/// ACP usage_update reports the latest context occupancy, not cumulative billing.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcpContextUsage {
    pub used: u64,
    pub size: u64,
}

pub(crate) fn parse_context_usage(update: &Value) -> Option<AcpContextUsage> {
    let used = update.get("used")?.as_u64()?;
    let size = update.get("size")?.as_u64()?;
    // JavaScript cannot faithfully represent larger integer token counts.
    (size > 0 && size <= 9_007_199_254_740_991 && used <= 9_007_199_254_740_991)
        .then_some(AcpContextUsage { used, size })
}

pub(crate) fn context_model(options: &[Value]) -> Option<&str> {
    options.iter().find(|option| option["id"] == "model")?.get("currentValue")?.as_str()
}

const CODEX_GOAL_METHOD: &str = "_session/goal";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodexGoal {
    pub objective: String,
    pub status: String,
    pub token_budget: Option<u64>,
    pub tokens_used: u64,
    pub time_used_seconds: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

pub(crate) fn parse_codex_goal(value: &Value) -> Option<CodexGoal> {
    let goal: CodexGoal = serde_json::from_value(value.clone()).ok()?;
    (!goal.objective.trim().is_empty() && matches!(goal.status.as_str(), "active" | "paused" | "blocked" | "complete" | "limited"))
        .then_some(goal)
}

type GoalReplies = Arc<Mutex<HashMap<String, mpsc::SyncSender<Value>>>>;

#[derive(Default)]
struct PromptCollector {
    text: String,
}

impl PromptCollector {
    fn push_text(&mut self, text: &str) {
        self.text.push_str(text);
    }
}

pub(crate) struct AcpConnection {
    child: Child,
    _process_identity: ProcessIdentityGuard,
    stdin: Arc<Mutex<ChildStdin>>,
    messages: mpsc::Receiver<String>,
    stderr: Arc<Mutex<String>>,
    next_id: u64,
    session_id: String,
    cwd: String,
    agent_name: String,
    agent_version: String,
    full_permissions: bool,
    auto_approve_permissions: bool,
    single_use_permissions: bool,
    read_only_crewvela_mcp_calls: HashSet<String>,
    last_start_resumed: bool,
    commands: Vec<AcpCommand>,
    process_id: Arc<AtomicU32>,
    permission_handler: Option<ManagedPermissionHandler>,
    config_options: Vec<Value>,
    context_usage: Option<AcpContextUsage>,
    context_usage_reported: bool,
    goal_supported: bool,
    plugin_supported: bool,
    goal_reported: bool,
    goal: Option<CodexGoal>,
    goal_replies: GoalReplies,
    adapter_kind: ManagedAcpKind,
    _runtime_lease: crate::runtime_preferences::RuntimeLease,
    tool_calls: HashMap<String, Value>,
    update_handler: Option<Box<dyn FnMut(&Value) + Send>>,
    native_control: Option<crate::harness_control::HarnessControl>,
}

static QUARANTINED_ACP: std::sync::LazyLock<Mutex<Vec<AcpConnection>>> = std::sync::LazyLock::new(|| Mutex::new(Vec::new()));
fn quarantine_acp_connection(connection: AcpConnection) {
    match QUARANTINED_ACP.lock() {
        Ok(mut owners) => owners.push(connection),
        Err(_) => std::mem::forget(connection), // Never run unverified Drop cleanup.
    }
}

pub(crate) type ManagedPermissionHandler =
    Box<dyn FnMut(&Value, Option<&AtomicBool>) -> Result<bool> + Send>;

impl Drop for AcpConnection {
    fn drop(&mut self) {
        if let Ok(mut replies) = self.goal_replies.lock() { replies.clear(); }
        if let Err(error) = self.stop_checked() {
            // Keep Atomic PID and the registered original identity on failure.
            eprintln!("[crewvela] ACP cleanup未确停，保留原始进程身份：{error:#}");
        }
    }
}

impl AcpConnection {
    fn spawn(
        cwd: &str,
        resume_session_id: Option<&str>,
        process_id: Arc<AtomicU32>,
    ) -> Result<Self> {
        Self::spawn_for(
            ManagedAcpKind::ClaudeCode,
            cwd,
            resume_session_id,
            Vec::new(),
            &["bypassPermissions"],
            true,
            false,
            process_id,
        )
    }

    fn spawn_for(
        adapter: ManagedAcpKind,
        cwd: &str,
        resume_session_id: Option<&str>,
        mcp_servers: Vec<Value>,
        mode_candidates: &[&str],
        auto_approve_permissions: bool,
        strict_mode: bool,
        process_id: Arc<AtomicU32>,
    ) -> Result<Self> {
        Self::spawn_for_with_cli_identity(
            adapter,
            cwd,
            resume_session_id,
            mcp_servers,
            mode_candidates,
            auto_approve_permissions,
            strict_mode,
            process_id,
            None,
            None,
            None,
        )
    }

    /// 原生 Claude Code 聊天会话：标准 ACP 适配器 + default 权限模式（审批卡片）。
    /// 未协商到 mode 时容忍受限模式继续（strict=false），与交互式会话语义一致。
    pub(crate) fn spawn_native_claude_chat(
        cwd: &str,
        resume_session_id: Option<&str>,
        mcp_servers: Vec<Value>,
        permission_handler: ManagedPermissionHandler,
        process_id: Arc<AtomicU32>,
        command_override: Option<Command>,
    ) -> Result<Self> {
        let mut connection = Self::spawn_for_with_cli_identity(
            ManagedAcpKind::ClaudeCode,
            cwd,
            resume_session_id,
            mcp_servers,
            &["default"],
            false,
            false,
            process_id,
            None,
            command_override,
            Some(crate::multi_agent::AgentRuntimeKind::ClaudeChat),
        )?;
        connection.permission_handler = Some(permission_handler);
        Ok(connection)
    }

    /// 原生 Kimi Code 聊天会话：kimi CLI 自带 ACP server（`kimi acp` 子命令），
    /// 无需任何适配器；default 权限模式（审批卡片），未协商到 mode 时容忍受限继续。
    pub(crate) fn spawn_native_kimi_chat(
        cwd: &str,
        resume_session_id: Option<&str>,
        mcp_servers: Vec<Value>,
        permission_handler: ManagedPermissionHandler,
        process_id: Arc<AtomicU32>,
        command_override: Option<Command>,
    ) -> Result<Self> {
        let command = match command_override {
            Some(command) => command,
            None => kimi_acp_command()?,
        };
        let mut connection = Self::spawn_for_with_cli_identity(
            ManagedAcpKind::Kimi,
            cwd,
            resume_session_id,
            mcp_servers,
            &["default"],
            false,
            false,
            process_id,
            None,
            Some(command),
            Some(crate::multi_agent::AgentRuntimeKind::KimiChat),
        )?;
        connection.permission_handler = Some(permission_handler);
        Ok(connection)
    }

    /// 原生 Codex 聊天会话：codex-acp 适配器驱动本机 codex CLI（`codex app-server`）。
    /// agent 权限模式（审批卡片），未协商到 mode 时容忍受限继续。
    pub(crate) fn spawn_native_codex_chat(
        cwd: &str,
        resume_session_id: Option<&str>,
        mcp_servers: Vec<Value>,
        permission_handler: ManagedPermissionHandler,
        process_id: Arc<AtomicU32>,
        command_override: Option<Command>,
    ) -> Result<Self> {
        let command = match command_override {
            Some(command) => command,
            None => {
                let mut command = adapter_command(ManagedAcpKind::Codex)?;
                if let Err(error) = crate::ai_memory::ensure_codex_service() {
                    // Shared memory is optional: report its failure without
                    // preventing Codex itself from opening a chat.
                    eprintln!("[crewvela] Codex Chat 共享记忆启动检查失败：{error:#}");
                }
                // 适配器默认 spawn 裸 "codex"，shell 解析失败时会回退包内旧版 codex
                // （0.153.x，模型目录陈旧）；显式指向本机解析到的 codex，与 PTY
                // 会话保持同一二进制与模型目录。
                if let Some(codex) = crate::multi_agent::resolve_runtime_executable(
                    crate::multi_agent::AgentRuntimeKind::Codex,
                ) {
                    command.env("CODEX_PATH", codex);
                }
                command
            }
        };
        let mut connection = Self::spawn_for_with_cli_identity(
            ManagedAcpKind::Codex,
            cwd,
            resume_session_id,
            mcp_servers,
            &["agent"],
            false,
            false,
            process_id,
            None,
            Some(command),
            Some(crate::multi_agent::AgentRuntimeKind::CodexChat),
        )?;
        connection.permission_handler = Some(permission_handler);
        Ok(connection)
    }

    /// 原生 OpenCode 聊天会话：opencode CLI 自带 ACP server（`opencode acp` 子命令），
    /// 无需任何适配器；权限走 opencode 逐工具审批（审批卡片），mode 由其自身默认 agent 决定。
    pub(crate) fn spawn_native_opencode_chat(
        cwd: &str,
        resume_session_id: Option<&str>,
        mcp_servers: Vec<Value>,
        permission_handler: ManagedPermissionHandler,
        process_id: Arc<AtomicU32>,
        command_override: Option<Command>,
    ) -> Result<Self> {
        let command = match command_override {
            Some(command) => command,
            None => opencode_acp_command()?,
        };
        let mut connection = Self::spawn_for_with_cli_identity(
            ManagedAcpKind::OpenCode,
            cwd,
            resume_session_id,
            mcp_servers,
            // opencode 的 mode 是其 agent 体系（build/plan 等），与 CrewVela 权限模式
            // 候选不同名；传空跳过协商，保持 opencode 默认 agent + 逐工具审批。
            &[],
            false,
            false,
            process_id,
            None,
            Some(command),
            Some(crate::multi_agent::AgentRuntimeKind::OpenCodeChat),
        )?;
        connection.permission_handler = Some(permission_handler);
        Ok(connection)
    }

    pub(crate) fn spawn_native_deepseek_harness(
        cwd: &str,
        resume_session_id: Option<&str>,
        mcp_servers: Vec<Value>,
        permission_handler: ManagedPermissionHandler,
        process_id: Arc<AtomicU32>,
        command_override: Option<Command>,
    ) -> Result<Self> {
        let mut command = match command_override { Some(command) => command, None => adapter_command(ManagedAcpKind::DeepSeekHarness)? };
        let native_control = crate::harness_control::HarnessControl::configure(&mut command)?;
        let mut connection = Self::spawn_for_with_cli_identity(
            ManagedAcpKind::DeepSeekHarness,
            cwd,
            resume_session_id,
            mcp_servers,
            &["workspace-write"],
            false,
            true,
            process_id,
            None,
            Some(command),
            None,
        )?;
        connection.permission_handler = Some(permission_handler);
        connection.native_control = Some(native_control);
        Ok(connection)
    }

    pub(crate) fn spawn_for_with_cli_identity(
        adapter: ManagedAcpKind,
        cwd: &str,
        resume_session_id: Option<&str>,
        mcp_servers: Vec<Value>,
        mode_candidates: &[&str],
        auto_approve_permissions: bool,
        strict_mode: bool,
        process_id: Arc<AtomicU32>,
        app_session_id: Option<&str>,
        command_override: Option<Command>,
        lease_kind_override: Option<crate::multi_agent::AgentRuntimeKind>,
    ) -> Result<Self> {
        Self::spawn_for_with_cli_identity_policy(adapter, cwd, resume_session_id, mcp_servers,
            mode_candidates, auto_approve_permissions, strict_mode, process_id, app_session_id,
            command_override, lease_kind_override, None)
    }

    fn spawn_for_with_cli_identity_policy(
        adapter: ManagedAcpKind,
        cwd: &str,
        resume_session_id: Option<&str>,
        mcp_servers: Vec<Value>,
        mode_candidates: &[&str],
        auto_approve_permissions: bool,
        strict_mode: bool,
        process_id: Arc<AtomicU32>,
        app_session_id: Option<&str>,
        command_override: Option<Command>,
        lease_kind_override: Option<crate::multi_agent::AgentRuntimeKind>,
        research: Option<ManagedResearchPolicy>,
    ) -> Result<Self> {
        if research.is_some() && (adapter != ManagedAcpKind::ClaudeCode || resume_session_id.is_some()) {
            return Err(anyhow!("此 Runtime 暂不能强制执行完整群聊工具白名单；受控研究需要独立的 Claude ACP 会话，不能降级为宽松权限"));
        }
        // 默认租约跟随适配器对应的 PTY Runtime（辅助进程语义）；四个原生聊天
        // 持独立启停开关，由聊天包装显式传入 chat kind 覆盖。
        let kind = lease_kind_override.unwrap_or(match adapter {
            ManagedAcpKind::ClaudeCode => crate::multi_agent::AgentRuntimeKind::ClaudeCode,
            ManagedAcpKind::Codex => crate::multi_agent::AgentRuntimeKind::Codex,
            ManagedAcpKind::DeepSeekHarness => {
                crate::multi_agent::AgentRuntimeKind::DeepSeekHarness
            }
            ManagedAcpKind::Kimi => crate::multi_agent::AgentRuntimeKind::KimiCode,
            ManagedAcpKind::OpenCode => crate::multi_agent::AgentRuntimeKind::OpenCode,
        });
        let runtime_lease = crate::runtime_preferences::acquire(kind)?;
        // Harness traverses workspace parents through Node realpath.native, which
        // rejects a Windows verbatim drive root (e.g. \\?\G:\).
        let harness_cwd;
        let cwd = if adapter == ManagedAcpKind::DeepSeekHarness {
            harness_cwd = canonical_harness_directory(cwd)?;
            harness_cwd.as_str()
        } else {
            cwd
        };
        let work_dir = Path::new(cwd);
        if !work_dir.is_dir() {
            return Err(anyhow!("请选择存在的工作目录后再启动 ACP 会话"));
        }

        let mut command = match command_override {
            Some(command) => command,
            None => adapter_command(adapter)?,
        };
        if research.is_some() {
            // Isolation must not silently switch the user's existing model provider.
            // Import only inert authentication/model environment values, never hooks,
            // plugins, arbitrary settings or credential helper commands.
            let directory = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from)
                .or_else(|| dirs::home_dir().map(|root| root.join(".claude")))
                .context("无法找到 Claude 用户认证配置目录")?;
            let path = directory.join("settings.json");
            match std::fs::read_to_string(&path) {
                Ok(source) => {
                    let settings: Value = serde_json::from_str(source.trim_start_matches('\u{feff}')).context("Claude 用户配置不是有效JSON；未使用不同供应商兜底")?;
                    command.envs(research_provider_env(&settings)?);
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {},
                Err(error) => return Err(error).context("读取 Claude 既有认证配置失败，未改变供应商"),
            }
        }
        if adapter == ManagedAcpKind::DeepSeekHarness && !mode_candidates.is_empty() {
            let mode = match mode_candidates.first().copied() {
                Some("read-only") => "read-only",
                Some("agent-full-access") if auto_approve_permissions => "danger-full-access",
                _ => "workspace-write",
            };
            command.env("DSH_PERMISSION_MODE", mode);
        }
        if let Some(app_session_id) = app_session_id.filter(|_| research.is_none()) {
            configure_session_cli_identity(&mut command, app_session_id)?;
            if adapter == ManagedAcpKind::Codex {
                configure_codex_acp_cli_identity(&mut command, app_session_id)?;
            }
        }
        command
            .current_dir(work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }

        terminate_acp_process_checked(&process_id)?;
        let (mut child, process_identity) = spawn_owned_command(&mut command)
            .with_context(|| format!("启动 {} ACP 适配器失败", adapter.label()))?;
        process_id.store(child.id(), Ordering::Release);
        let stdin = Arc::new(Mutex::new(
            child.stdin.take().context("ACP 适配器没有可写入的 stdin")?,
        ));
        let stdout = child
            .stdout
            .take()
            .context("ACP 适配器没有可读取的 stdout")?;
        let stderr_pipe = child.stderr.take().context("ACP 适配器没有 stderr")?;
        let (sender, messages) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                match line {
                    Ok(line) if !line.trim().is_empty() => {
                        if sender.send(line).is_err() {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => break,
                }
            }
        });
        let stderr = capture_stderr(stderr_pipe);

        let mut connection = Self {
            child,
            _process_identity: process_identity,
            stdin,
            messages,
            stderr,
            next_id: 0,
            session_id: String::new(),
            cwd: cwd.to_string(),
            agent_name: format!("{} Agent", adapter.label()),
            agent_version: String::new(),
            full_permissions: false,
            auto_approve_permissions,
            read_only_crewvela_mcp_calls: HashSet::new(),
            last_start_resumed: false,
            commands: Vec::new(),
            process_id,
            permission_handler: None,
            single_use_permissions: false,
            config_options: Vec::new(),
            context_usage: None,
            context_usage_reported: false,
            goal_supported: false,
            plugin_supported: false,
            goal_reported: false,
            goal: None,
            goal_replies: Arc::new(Mutex::new(HashMap::new())),
            adapter_kind: adapter,
            _runtime_lease: runtime_lease,
            tool_calls: HashMap::new(),
            update_handler: None,
            native_control: None,
        };

        let configured = (|| -> Result<()> {
        let initialized = connection.request(
            "initialize",
            json!({
                "protocolVersion": 1,
                "clientCapabilities": if adapter == ManagedAcpKind::Codex {
                    // codex-acp 1.11 otherwise renders provider failures as assistant text
                    // and returns end_turn. Negotiate its versioned terminal result instead.
                    json!({"_meta":{"jetbrains":{"air":{"version":1,"capabilities":["sessionFailure"]}}}})
                } else { json!({}) },
                "clientInfo": {
                    "name": "crewvela",
                    "title": "CrewVela",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
            START_TIMEOUT,
            None,
        )?;
        let supports_http_mcp = initialized
            .pointer("/agentCapabilities/mcpCapabilities/http")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        connection.goal_supported = adapter == ManagedAcpKind::Codex
            && initialized.pointer("/_meta/crewvelaGoal/nativeSubscriptions").and_then(Value::as_bool) == Some(true)
            && initialized.pointer("/_meta/goal/version").and_then(Value::as_u64) == Some(1)
            && initialized.pointer("/_meta/goal/controlMethod").and_then(Value::as_str) == Some(CODEX_GOAL_METHOD)
            && initialized.pointer("/_meta/goal/actions").and_then(Value::as_array).is_some_and(|actions| {
                ["set", "pause", "resume", "clear"].iter().all(|action| actions.iter().any(|value| value.as_str() == Some(*action)))
            });
        connection.plugin_supported = adapter == ManagedAcpKind::Codex
            && initialized.pointer("/_meta/crewvelaPlugins/version").and_then(Value::as_u64) == Some(1)
            && initialized.pointer("/_meta/crewvelaPlugins/nativeMentions").and_then(Value::as_bool) == Some(true);
        // 只有 http 条目（如 crewvela 控制面）需要能力协商；stdio 条目
        // （浏览器控制等本地 server）由 Agent 直接 spawn，不受 http 能力限制。
        let has_http_mcp_entries = mcp_servers
            .iter()
            .any(|server| server.get("type").and_then(Value::as_str) == Some("http"));
        if has_http_mcp_entries && !supports_http_mcp {
            return Err(anyhow!(
                "{} ACP Adapter 不支持 HTTP MCP，无法连接 CrewVela 控制面",
                adapter.label()
            ));
        }
        let negotiated = initialized
            .get("protocolVersion")
            .and_then(Value::as_u64)
            .unwrap_or_default();
        if negotiated != 1 {
            return Err(anyhow!("ACP 协议版本不兼容：Agent 返回版本 {negotiated}"));
        }
        if let Some(info) = initialized.get("agentInfo") {
            connection.agent_name = info
                .get("title")
                .or_else(|| info.get("name"))
                .and_then(Value::as_str)
                .unwrap_or_else(|| adapter.label())
                .to_string();
            connection.agent_version = info
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
        }
        if research.is_some() && connection.agent_version != "0.63.0" {
            return Err(anyhow!("Claude ACP {} 尚未验证群聊工具白名单契约（需要 0.63.0），拒绝扩大权限", connection.agent_version));
        }

        let can_load = initialized
            .pointer("/agentCapabilities/loadSession")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let loaded_session = if adapter == ManagedAcpKind::DeepSeekHarness {
            match resume_session_id.filter(|id| !id.trim().is_empty()) {
                Some(session_id) => {
                    if initialized
                        .pointer("/agentCapabilities/sessionCapabilities/resume")
                        .is_none()
                    {
                        return Err(anyhow!(
                            "DeepSeek Harness 不支持 session/resume，不能恢复已有会话"
                        ));
                    }
                    let mut session = connection.request("session/resume",
                        json!({"sessionId": session_id, "cwd": cwd, "mcpServers": mcp_servers.clone()}),
                        START_TIMEOUT, None)?;
                    session
                        .as_object_mut()
                        .context("ACP resume 返回值不是对象")?
                        .insert("sessionId".into(), Value::String(session_id.into()));
                    Some(session)
                }
                None => None,
            }
        } else if can_load {
            resume_session_id
                .filter(|value| !value.trim().is_empty())
                .and_then(|session_id| {
                    connection
                        .request(
                            "session/load",
                            { let mut params=managed_session_params(adapter,cwd,&mcp_servers,research,mode_candidates.is_empty());params["sessionId"]=json!(session_id);params },
                            START_TIMEOUT,
                            None,
                        )
                        .ok()
                        .map(|mut session| {
                            // 部分适配器（codex-acp）的 load 响应不带 sessionId：
                            // load 已成功，按请求的恢复 ID 回填，与 dsh resume 路径同语义。
                            if session.get("sessionId").and_then(Value::as_str).is_none() {
                                if let Some(object) = session.as_object_mut() {
                                    object.insert("sessionId".into(), Value::String(session_id.to_string()));
                                }
                            }
                            session
                        })
                })
        } else {
            None
        };
        let resumed = loaded_session.is_some();
        let session = match loaded_session {
            Some(session) => session,
            None => connection.request(
                "session/new",
                managed_session_params(adapter,cwd,&mcp_servers,research,mode_candidates.is_empty()),
                START_TIMEOUT,
                None,
            )?,
        };
        connection.session_id = session
            .get("sessionId")
            .and_then(Value::as_str)
            .context("ACP Agent 未返回 sessionId")?
            .to_string();
        connection.config_options = session
            .get("configOptions")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();

        let available_modes = session
            .pointer("/modes/availableModes")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let current_mode = session.pointer("/modes/currentModeId").and_then(Value::as_str);
        let current_full_access = matches!(current_mode, Some("bypassPermissions" | "agent-full-access"));
        let negotiated_mode =
            if adapter == ManagedAcpKind::DeepSeekHarness || mode_candidates.is_empty() {
                None
            } else {
                let mode_id = mode_candidates
                    .iter()
                    .find(|candidate| {
                        available_modes
                            .iter()
                            .any(|mode| mode.get("id").and_then(Value::as_str) == Some(**candidate))
                    })
                    .copied();
                match (mode_id, strict_mode) {
                    (Some(mode_id), _) => Some(mode_id),
                    (None, true) => {
                        return Err(anyhow!(
                            "{} ACP Adapter 不支持所需权限模式",
                            adapter.label()
                        ));
                    }
                    // 交互式会话容忍模式缺失：保持受限模式继续，而不是启动失败
                    (None, false) if current_full_access => {
                        return Err(anyhow!("{} ACP Adapter 当前处于完全权限模式且不支持请求的受限模式，已拒绝继续", adapter.label()));
                    }
                    (None, false) => None,
                }
            };
        if let Some(mode_id) = negotiated_mode {
            if current_mode != Some(mode_id) {
                let session_id = connection.session_id.clone();
                let set_mode = connection.request(
                    "session/set_mode",
                    json!({ "sessionId": session_id, "modeId": mode_id }),
                    START_TIMEOUT,
                    None,
                );
                if strict_mode || current_full_access {
                    set_mode.context("ACP 权限模式切换失败，无法确认已降权，已拒绝继续")?;
                    connection.full_permissions =
                        matches!(mode_id, "bypassPermissions" | "agent-full-access");
                } else {
                    // 仅已知当前非完全权限时允许交互会话保留原权限模式。
                    connection.full_permissions = set_mode.is_ok()
                        && matches!(mode_id, "bypassPermissions" | "agent-full-access");
                }
            } else {
                connection.full_permissions =
                    matches!(mode_id, "bypassPermissions" | "agent-full-access");
            }
        }
        connection.drain_updates(Duration::from_millis(750), None)?;
        connection.ensure_clear_command();
        connection.last_start_resumed = resumed;
        Ok(())
        })();
        if let Err(error) = configured {
            if let Err(stop_error) = connection.stop_checked() {
                // Keep the whole owner alive. The registry also retains its root
                // identity, so cleanup failure cannot become a fallback launch.
                quarantine_acp_connection(connection);
                return Err(stop_error);
            }
            return Err(error);
        }
        Ok(connection)
    }

    fn status(&mut self, resumed: bool) -> Result<AcpStatus> {
        self.drain_updates(Duration::from_millis(20), None)?;
        self.ensure_clear_command();
        Ok(self.snapshot(resumed))
    }

    fn prompt(&mut self, message: &str) -> Result<AcpPromptResult> {
        self.prompt_cancellable(message, None)
    }

    fn prompt_cancellable(
        &mut self,
        message: &str,
        cancellation: Option<&AtomicBool>,
    ) -> Result<AcpPromptResult> {
        let message = message.trim();
        if message.is_empty() {
            return Err(anyhow!("ACP 消息不能为空"));
        }
        self.prompt_content_cancellable(vec![json!({"type":"text","text":message})], cancellation)
    }

    fn prompt_content_cancellable(
        &mut self,
        content: Vec<Value>,
        cancellation: Option<&AtomicBool>,
    ) -> Result<AcpPromptResult> {
        let session_id = self.session_id.clone();
        let mut collector = PromptCollector::default();
        let response = self.request_cancellable(
            "session/prompt",
            json!({
                "sessionId": session_id,
                "prompt": content
            }),
            PROMPT_TIMEOUT,
            Some(&mut collector),
            cancellation,
        )?;
        let cancelled = response.get("stopReason").and_then(Value::as_str) == Some("cancelled");
        self.drain_updates(Duration::from_millis(120), Some(&mut collector))?;
        self.ensure_clear_command();
        if !cancelled && self.adapter_kind == ManagedAcpKind::Codex {
            if let Some(failure) = response.pointer("/_meta/jetbrains/air/sessionFailure") {
                if failure.get("severity").and_then(Value::as_str) == Some("error") {
                    let title = failure.get("title").and_then(Value::as_str).unwrap_or("未返回失败详情");
                    // Only decode a typed failure's title, never scan legitimate assistant text.
                    let payload = serde_json::from_str::<Value>(title).ok();
                    let detail = payload.as_ref().and_then(|value| value.pointer("/error/message"))
                        .and_then(Value::as_str).unwrap_or(title);
                    let code = payload.as_ref().and_then(|value| value.pointer("/error/code"))
                        .and_then(Value::as_str).or_else(|| failure.get("category").and_then(Value::as_str))
                        .unwrap_or("unknown");
                    return Err(anyhow!("Codex 请求失败（{code}）：{detail}"));
                }
            }
        }
        Ok(AcpPromptResult {
            text: collector.text.trim().to_string(),
            commands: self.commands.clone(),
            cancelled,
        })
    }

    pub(crate) fn set_update_handler(&mut self, handler: Box<dyn FnMut(&Value) + Send>) {
        self.update_handler = Some(handler);
    }

    pub(crate) fn config_options(&self) -> &[Value] {
        &self.config_options
    }

    pub(crate) fn context_usage(&self) -> Option<Option<AcpContextUsage>> {
        self.context_usage_reported.then(|| self.context_usage.clone())
    }

    pub(crate) fn goal(&self) -> Option<Option<CodexGoal>> {
        self.goal_reported.then(|| self.goal.clone())
    }

    pub(crate) fn goal_handle(&self) -> Option<CodexGoalHandle> {
        self.goal_supported.then(|| CodexGoalHandle { stdin: Arc::clone(&self.stdin), session_id: self.session_id.clone(), replies: Arc::clone(&self.goal_replies) })
    }

    pub(crate) fn commands(&self) -> &[AcpCommand] {
        &self.commands
    }

    /// 本次 spawn 是否通过 session/load 恢复了上游会话（而非回退 session/new）。
    pub(crate) fn resumed(&self) -> bool {
        self.last_start_resumed
    }

    pub(crate) fn cancel_handle(&self) -> AcpCancelHandle {
        AcpCancelHandle {
            stdin: Arc::clone(&self.stdin),
            session_id: self.session_id.clone(),
        }
    }

    pub(crate) fn set_deepseek_model(&mut self, value: &str) -> Result<Vec<Value>> {
        let selected = resolve_deepseek_model(&self.config_options, value)?;
        self.set_config_option("model", &selected)
    }

    pub(crate) fn set_config_option(&mut self, id: &str, value: &str) -> Result<Vec<Value>> {
        let root = self.config_options.iter().find(|option| option["id"] == id).context("ACP Agent 未公开此配置选项")?;
        fn contains(options: &Value, value: &str) -> bool {
            options.as_array().is_some_and(|options| options.iter().any(|option| option["value"] == value || contains(&option["options"], value)))
        }
        if !contains(&root["options"], value) { return Err(anyhow!("ACP 配置值不在已公开选项中")); }
        let result = self.request(
            "session/set_config_option",
            json!({"sessionId": self.session_id, "configId": id, "value": value}),
            START_TIMEOUT,
            None,
        )?;
        if let Some(options) = result.get("configOptions").and_then(Value::as_array) {
            self.config_options = options.clone();
        }
        Ok(self.config_options.clone())
    }

    pub(crate) fn prompt_interactive(
        &mut self,
        message: &str,
        cancellation: &AtomicBool,
    ) -> Result<AcpPromptResult> {
        self.prompt_cancellable(message, Some(cancellation))
    }

    pub(crate) fn native_control(&self, action: &str, value: &str) -> Result<Value> {
        self.native_control.as_ref().context("Harness 控制插件未连接")?.request(&self.session_id, action, value)
    }

    pub(crate) fn native_control_handle(&self) -> Option<crate::harness_control::HarnessControl> { self.native_control.clone() }

    pub(crate) fn poll_native_updates(&mut self) -> Result<()> {
        self.drain_updates(Duration::from_millis(5), None)
    }

    pub(crate) fn codex_plugins(&mut self) -> Result<Value> {
        if !self.plugin_supported { return Err(anyhow!("当前 Codex 适配器不支持原生插件引用；请使用 CrewVela 锁定的适配器")); }
        self.request("_crewvela/plugins", json!({"sessionId": self.session_id}), Duration::from_secs(30), None)
    }

    pub(crate) fn prompt_content_interactive(
        &mut self, content: Vec<Value>, cancellation: &AtomicBool,
    ) -> Result<AcpPromptResult> {
        self.prompt_content_cancellable(content, Some(cancellation))
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn cwd(&self) -> &str {
        &self.cwd
    }

    pub(crate) fn stop_checked(&mut self) -> Result<()> {
        if !self._process_identity.is_verified() { stop_child_checked(&mut self.child)?; }
        self.child.wait().map_err(|error| anyhow!(UnverifiedStop(format!("回收ACP首领失败：{error}"))))?;
        let _ = self.process_id.compare_exchange(self.child.id(), 0, Ordering::AcqRel, Ordering::Acquire);
        Ok(())
    }

    pub(crate) fn terminate(&self) {
        terminate_acp_process(&self.process_id);
    }


    pub(crate) fn has_exited(&mut self) -> Result<bool> {
        Ok(try_wait_child_preserving_identity(&mut self.child)?.is_some())
    }
    fn snapshot(&self, resumed: bool) -> AcpStatus {
        AcpStatus {
            connected: true,
            session_id: self.session_id.clone(),
            cwd: self.cwd.clone(),
            agent_name: self.agent_name.clone(),
            agent_version: self.agent_version.clone(),
            full_permissions: self.full_permissions,
            resumed,
            commands: self.commands.clone(),
            config_options: self.config_options.clone(),
        }
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
        collector: Option<&mut PromptCollector>,
    ) -> Result<Value> {
        self.request_cancellable(method, params, timeout, collector, None)
    }

    fn request_cancellable(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
        mut collector: Option<&mut PromptCollector>,
        cancellation: Option<&AtomicBool>,
    ) -> Result<Value> {
        let watch_opencode_failure = self.adapter_kind == ManagedAcpKind::OpenCode && method == "session/prompt";
        if watch_opencode_failure {
            // Previous turns/startup diagnostics must not reject a fresh prompt.
            self.stderr.lock().map_err(|_| anyhow!("OpenCode 错误状态锁损坏"))?.clear();
        }
        let id = self.next_id;
        self.next_id += 1;
        self.write(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params
        }))?;
        let started = Instant::now();
        loop {
            if cancellation.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                if !self.session_id.is_empty() {
                    self.write(cancel_notification(&self.session_id))?;
                }
                return Err(anyhow!("ACP Agent 任务已取消"));
            }
            if watch_opencode_failure {
                let failure = opencode_terminal_prompt_failure(
                    &self.stderr.lock().map_err(|_| anyhow!("OpenCode 错误状态锁损坏"))?, &self.session_id,
                );
                if let Some(message) = failure { return Err(anyhow!(message)); }
            }
            let remaining = timeout
                .checked_sub(started.elapsed())
                .ok_or_else(|| anyhow!("ACP 请求超时：{method}"))?;
            let wait = if cancellation.is_some() || watch_opencode_failure {
                remaining.min(Duration::from_millis(100))
            } else {
                remaining
            };
            let line = match self.messages.recv_timeout(wait) {
                Ok(line) => line,
                Err(mpsc::RecvTimeoutError::Timeout) if wait < remaining => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(anyhow!("ACP 请求超时：{method}"));
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("ACP 适配器已退出：{}", self.stderr_text()));
                }
            };
            let value: Value = serde_json::from_str(&line)
                .with_context(|| format!("ACP 返回了无效 JSON：{line}"))?;
            // 双向 JSON-RPC 各自分配请求 ID；同号的 Agent 权限请求不是本次响应。
            if value.get("method").is_none()
                && (value.get("result").is_some() || value.get("error").is_some())
                && value.get("id").and_then(Value::as_u64) == Some(id)
            {
                if let Some(error) = value.get("error") {
                    if error.get("code").and_then(Value::as_i64) == Some(-32800) {
                        return Ok(json!({ "stopReason": "cancelled" }));
                    }
                    return Err(anyhow!("ACP 请求失败：{error}"));
                }
                return Ok(value.get("result").cloned().unwrap_or(Value::Null));
            }
            self.handle_message(&value, collector.as_deref_mut(), cancellation)?;
        }
    }
    fn drain_updates(
        &mut self,
        idle: Duration,
        mut collector: Option<&mut PromptCollector>,
    ) -> Result<()> {
        loop {
            match self.messages.recv_timeout(idle) {
                Ok(line) => {
                    let value: Value = serde_json::from_str(&line)
                        .with_context(|| format!("ACP 返回了无效 JSON：{line}"))?;
                    self.handle_message(&value, collector.as_deref_mut(), None)?;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => return Ok(()),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(anyhow!("ACP 适配器已退出：{}", self.stderr_text()))
                }
            }
        }
    }

    fn handle_message(
        &mut self,
        value: &Value,
        collector: Option<&mut PromptCollector>,
        cancellation: Option<&AtomicBool>,
    ) -> Result<()> {
        if value.get("method").is_none() && (value.get("result").is_some() || value.get("error").is_some()) {
            if let Some(id) = value.get("id").and_then(Value::as_str) {
                if let Some(reply) = self.goal_replies.lock().map_err(|_| anyhow!("Codex 目标响应锁已损坏"))?.remove(id) {
                    let _ = reply.send(value.clone());
                    return Ok(());
                }
            }
        }
        if value.get("method").and_then(Value::as_str) == Some("session/request_permission")
            && value.get("id").is_some()
        {
            let mut enriched = value.clone();
            if let Some(id) = value
                .pointer("/params/toolCall/toolCallId")
                .and_then(Value::as_str)
            {
                if let Some(cached) = self.tool_calls.get(id).and_then(Value::as_object) {
                    if let Some(tool_call) = enriched
                        .pointer_mut("/params/toolCall")
                        .and_then(Value::as_object_mut)
                    {
                        for (key, detail) in cached {
                            tool_call
                                .entry(key.clone())
                                .or_insert_with(|| detail.clone());
                        }
                    }
                }
            }
            let allow = if let Some(handler) = self.permission_handler.as_mut() {
                handler(&enriched, cancellation)?
            } else {
                should_allow_permission(
                    value,
                    self.auto_approve_permissions,
                    &self.read_only_crewvela_mcp_calls,
                )
            };
            // 权限请求一旦答复即消费对应的只读标记，避免残留 ID 命中后续无关请求
            if let Some(tool_call_id) = value
                .pointer("/params/toolCall/toolCallId")
                .and_then(Value::as_str)
            {
                self.read_only_crewvela_mcp_calls.remove(tool_call_id);
            }
            let option = if allow && (self.adapter_kind == ManagedAcpKind::DeepSeekHarness || self.single_use_permissions) {
                value
                    .pointer("/params/options")
                    .and_then(Value::as_array)
                    .and_then(|options| {
                        options.iter().find(|option| {
                            option.get("kind").and_then(Value::as_str) == Some("allow_once")
                        })
                    })
                    .and_then(|option| option.get("optionId"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            } else {
                choose_permission_option(value, allow)
            };
            let outcome = match option {
                Some(option_id) => json!({ "outcome": "selected", "optionId": option_id }),
                // 没有匹配选项时回 cancelled（拒绝语义），而不是让请求报错或误选 allow
                None => json!({ "outcome": "cancelled" }),
            };
            return self.write(json!({
                "jsonrpc": "2.0",
                "id": value.get("id").cloned().unwrap_or(Value::Null),
                "result": { "outcome": outcome }
            }));
        }
        if value.get("method").and_then(Value::as_str) == Some("session/update") {
            if let Some(update) = value.pointer("/params/update") {
                self.apply_update(update, collector);
                if let Some(handler) = self.update_handler.as_mut() {
                    handler(update);
                }
            }
            return Ok(());
        }
        if value.get("method").is_some() && value.get("id").is_some() {
            self.write(json!({
                "jsonrpc": "2.0",
                "id": value.get("id").cloned().unwrap_or(Value::Null),
                "error": { "code": -32601, "message": "CrewVela 不支持该 ACP Client 方法" }
            }))?;
        }
        Ok(())
    }

    fn apply_update(&mut self, update: &Value, collector: Option<&mut PromptCollector>) {
        if self.adapter_kind == ManagedAcpKind::DeepSeekHarness
            && matches!(
                update.get("sessionUpdate").and_then(Value::as_str),
                Some("tool_call" | "tool_call_update")
            )
        {
            if let (Some(id), Some(details)) = (
                update.get("toolCallId").and_then(Value::as_str),
                update.as_object(),
            ) {
                let cached = self
                    .tool_calls
                    .entry(id.into())
                    .or_insert_with(|| json!({}));
                if let Some(cached) = cached.as_object_mut() {
                    cached.extend(details.clone());
                }
            }
        }
        if is_read_only_crewvela_mcp_call(update) {
            if let Some(tool_call_id) = update.get("toolCallId").and_then(Value::as_str) {
                self.read_only_crewvela_mcp_calls
                    .insert(tool_call_id.to_string());
            }
        }
        // 工具调用到达终态后释放只读标记，防止 ID 在连接生命周期内累积
        if update.get("sessionUpdate").and_then(Value::as_str) == Some("tool_call_update")
            && matches!(
                update.get("status").and_then(Value::as_str),
                Some("completed" | "failed")
            )
        {
            if let Some(tool_call_id) = update.get("toolCallId").and_then(Value::as_str) {
                self.read_only_crewvela_mcp_calls.remove(tool_call_id);
                self.tool_calls.remove(tool_call_id);
            }
        }
        match update.get("sessionUpdate").and_then(Value::as_str) {
            Some("session_info_update") if self.adapter_kind == ManagedAcpKind::Codex => {
                if let Some(goal) = update.pointer("/_meta/goal") {
                    self.goal_reported = true;
                    self.goal = parse_codex_goal(goal);
                }
            }
            Some("usage_update") if self.adapter_kind == ManagedAcpKind::Codex => {
                self.context_usage_reported = true;
                self.context_usage = parse_context_usage(update);
            }
            Some("available_commands_update") => {
                self.commands = parse_commands(update);
                self.ensure_clear_command();
            }
            Some("config_option_update") => {
                if let Some(options) = update.get("configOptions").and_then(Value::as_array) {
                    if self.adapter_kind == ManagedAcpKind::Codex
                        && context_model(&self.config_options) != context_model(options)
                    {
                        self.context_usage = None;
                        self.context_usage_reported = true;
                    }
                    self.config_options = options.clone();
                }
            }
            Some("agent_message_chunk") => {
                if let (Some(collector), Some(text)) = (
                    collector,
                    update.pointer("/content/text").and_then(Value::as_str),
                ) {
                    collector.push_text(text);
                }
            }
            _ => {}
        }
    }

    fn ensure_clear_command(&mut self) {
        if self.adapter_kind == ManagedAcpKind::DeepSeekHarness {
            return;
        }
        if self.commands.iter().any(|command| command.name == "clear") {
            return;
        }
        self.commands.push(AcpCommand {
            name: "clear".to_string(),
            description: "清空当前 Claude Code 上下文并开始新会话".to_string(),
            input_hint: None,
        });
        self.commands
            .sort_by(|left, right| left.name.cmp(&right.name));
    }

    fn write(&mut self, value: Value) -> Result<()> {
        write_to_stdin(&self.stdin, value)
    }

    fn stderr_text(&mut self) -> String {
        let status = try_wait_child_preserving_identity(&mut self.child).ok().flatten();
        let log = self
            .stderr
            .lock()
            .map(|value| value.trim().to_string())
            .unwrap_or_default();
        if self.adapter_kind == ManagedAcpKind::OpenCode {
            // --print-logs can include provider headers and URLs. Expose only
            // the classified failure, never the captured raw stderr buffer.
            let reason = opencode_terminal_prompt_failure(&log, &self.session_id)
                .unwrap_or("OpenCode ACP 连接已关闭，请检查本机 OpenCode 日志后恢复会话");
            return status.map_or_else(|| reason.to_string(), |status| format!("进程 {status}；{reason}"));
        }
        match (status, log.is_empty()) {
            (Some(status), false) => format!("进程 {status}；{log}"),
            (Some(status), true) => format!("进程 {status}"),
            (None, false) => log,
            (None, true) => "连接已关闭".to_string(),
        }
    }
}

/// Transfer must retain process identity until its tree termination is verified.
#[derive(Debug)]
pub(crate) struct UnverifiedStop(pub String);
impl std::fmt::Display for UnverifiedStop {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(formatter, "停止未获确认：{}", self.0) }
}
impl std::error::Error for UnverifiedStop {}

#[cfg(windows)]
struct ProcessIdentity {
    handle: std::os::windows::io::OwnedHandle,
    job: Option<Arc<ProcessJob>>,
    verified: AtomicBool,
    cleanup_failed: AtomicBool,
}
#[cfg(windows)]
static PROCESS_IDENTITIES: std::sync::LazyLock<Mutex<HashMap<u32, Arc<ProcessIdentity>>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) struct ProcessIdentityGuard {
    #[cfg(windows)]
    identity: Arc<ProcessIdentity>,
    #[cfg(unix)]
    group: Arc<process_group::GroupIdentity>,
}
impl ProcessIdentityGuard {
    pub(crate) fn is_verified(&self) -> bool {
        #[cfg(windows)] { self.identity.verified.load(Ordering::Acquire) }
        #[cfg(unix)] { self.group.verified.load(Ordering::Acquire) }
        #[cfg(not(any(windows, unix)))] { false }
    }
}
impl Drop for ProcessIdentityGuard {
    fn drop(&mut self) {
        #[cfg(unix)] {
            if let Err(error) = process_group::owner_dropped(&self.group) { eprintln!("[crewvela] 保留未确停进程组身份：{error:#}"); }
        }
        #[cfg(windows)] {
            use std::os::windows::io::AsHandle;
            if !self.identity.verified.load(Ordering::Acquire) && !self.identity.cleanup_failed.load(Ordering::Acquire) {
                if let Err(error) = stop_process_handle_checked(self.identity.handle.as_handle()) {
                    // Keep the registered original handle available after cleanup failure.
                    eprintln!("[crewvela] 保留未确停进程身份：{error:#}");
                }
            }
        }
    }
}

pub(crate) fn configure_process_identity(command: &mut Command) {
    #[cfg(unix)] {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(not(unix))] { let _ = command; }
}

#[cfg(windows)]
pub(crate) fn create_process_job()->Result<Arc<ProcessJob>> { process_job::ProcessJob::new().map(Arc::new) }

#[cfg(windows)]
pub(crate) fn register_process_handle_with_job(root:std::os::windows::io::BorrowedHandle<'_>,job:Arc<ProcessJob>)->Result<ProcessIdentityGuard> {
    use std::os::windows::io::AsRawHandle;
    let pid=unsafe { windows_sys::Win32::System::Threading::GetProcessId(root.as_raw_handle()) };
    if pid==0 || !job.contains(root)? { return Err(anyhow!(UnverifiedStop("进程没有在执行前进入指定Job".into()))); }
    let handle=root.try_clone_to_owned().context("复制Job根进程身份失败")?;
    let identity=Arc::new(ProcessIdentity {handle,job:Some(job),verified:AtomicBool::new(false),cleanup_failed:AtomicBool::new(false)});
    let mut identities=PROCESS_IDENTITIES.lock().map_err(|_|anyhow!(UnverifiedStop("进程身份锁异常".into())))?;
    if identities.contains_key(&pid){return Err(anyhow!(UnverifiedStop(format!("PID {pid}已存在未释放身份"))));}
    identities.insert(pid,Arc::clone(&identity));Ok(ProcessIdentityGuard{identity})
}

pub(crate) fn spawn_owned_command(command:&mut Command)->Result<(Child,ProcessIdentityGuard)> {
    #[cfg(windows)] {
        use std::os::windows::io::AsHandle;
        let job=create_process_job()?;
        let mut child=process_job::spawn_suspended(command)?;
        if let Err(error)=job.assign(child.as_handle()) {
            process_job::cleanup_unresumed(&mut child).map_err(|cleanup|anyhow!(UnverifiedStop(format!("PID {} Job分配失败且暂停进程未确停：{error:#}；{cleanup:#}",child.id()))))?;
            return Err(error);
        }
        let identity=match register_process_handle_with_job(child.as_handle(),Arc::clone(&job)) {
            Ok(identity)=>identity,
            Err(error)=>{job.stop().map_err(|cleanup|anyhow!(UnverifiedStop(format!("Job身份登记失败且未确停：{error:#}；{cleanup:#}"))))?;child.wait()?;return Err(error);}
        };
        if let Err(error)=process_job::resume_initial_thread(child.as_handle()) {
            stop_child_checked(&mut child)?;return Err(error);
        }
        Ok((child,identity))
    }
    #[cfg(not(windows))] {
        configure_process_identity(command);
        let mut child=command.spawn().context("启动受管进程失败")?;
        let identity=match register_process_identity(&child){Ok(identity)=>identity,Err(error)=>{stop_child_checked(&mut child)?;return Err(error);}};
        Ok((child,identity))
    }
}

pub(crate) fn register_process_identity(child: &Child) -> Result<ProcessIdentityGuard> {
    #[cfg(windows)] {
        use std::os::windows::io::AsHandle;
        let handle = child.as_handle().try_clone_to_owned().context("复制原生进程身份句柄失败")?;
        let identity = Arc::new(ProcessIdentity { handle, job: None, verified: AtomicBool::new(false), cleanup_failed: AtomicBool::new(false) });
        let mut identities = PROCESS_IDENTITIES.lock().map_err(|_| anyhow!("进程身份锁异常"))?;
        if identities.contains_key(&child.id()) { return Err(anyhow!(UnverifiedStop(format!("PID {} 已有未释放身份，拒绝替换", child.id())))); }
        identities.insert(child.id(), Arc::clone(&identity));
        Ok(ProcessIdentityGuard { identity })
    }
    #[cfg(unix)] { register_process_group_identity(child.id()) }
    #[cfg(not(any(windows, unix)))] { let _ = child; Ok(ProcessIdentityGuard {}) }
}

#[cfg(unix)]
pub(crate) fn register_process_group_identity(pid: u32) -> Result<ProcessIdentityGuard> {
    process_group::register(pid).map(|group| ProcessIdentityGuard { group })
}
#[cfg(unix)]
pub(crate) fn peek_process_exit(pid: u32) -> std::io::Result<Option<std::process::ExitStatus>> { process_group::peek_process_exit(pid) }

pub(crate) fn try_wait_child_preserving_identity(child: &mut Child) -> std::io::Result<Option<std::process::ExitStatus>> {
    #[cfg(unix)] { peek_process_exit(child.id()) }
    #[cfg(not(unix))] { child.try_wait() }
}

pub(crate) fn stop_child_checked(child: &mut Child) -> Result<()> {
    #[cfg(windows)] {
        use std::os::windows::io::AsHandle;
        stop_process_handle_checked(child.as_handle())?;
        child.wait().map_err(|error| anyhow!(UnverifiedStop(format!("等待原生进程退出失败：{error}"))))?;
        Ok(())
    }
    #[cfg(unix)] {
        process_group::stop_registered(child.id())?;
        child.wait().map_err(|error| anyhow!(UnverifiedStop(format!("等待原生组首领退出失败：{error}"))))?;
        Ok(())
    }
    #[cfg(not(any(windows, unix)))] { let _ = child; Err(anyhow!(UnverifiedStop("当前平台无法确证原生进程树停止，保留原执行记录".into()))) }
}

static PROCESS_STOP_GATES: std::sync::LazyLock<Mutex<HashMap<usize, std::sync::Weak<Mutex<()>>>>> = std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));

pub(crate) fn terminate_acp_process_checked(process_id: &AtomicU32) -> Result<()> {
    let gate = {
        let mut gates = PROCESS_STOP_GATES.lock().map_err(|_| anyhow!(UnverifiedStop("停止状态锁异常".into())))?;
        gates.retain(|_, gate| gate.strong_count() != 0);
        let key = process_id as *const AtomicU32 as usize;
        match gates.get(&key).and_then(std::sync::Weak::upgrade) {
            Some(gate) => gate,
            None => { let gate = Arc::new(Mutex::new(())); gates.insert(key, Arc::downgrade(&gate)); gate },
        }
    };
    let _guard = gate.lock().map_err(|_| anyhow!(UnverifiedStop("停止状态锁异常".into())))?;
    let pid = process_id.load(Ordering::Acquire);
    if pid == 0 { return Ok(()); }
    terminate_process_tree_checked(pid)?;
    let _ = process_id.compare_exchange(pid, 0, Ordering::AcqRel, Ordering::Acquire);
    Ok(())
}

/// PID-only callers must resolve a spawn-time owned identity, never reopen a PID.
pub(crate) fn terminate_process_tree_checked(pid: u32) -> Result<()> {
    #[cfg(windows)] {
        use std::os::windows::io::AsHandle;
        let identity = PROCESS_IDENTITIES.lock().map_err(|_| anyhow!(UnverifiedStop("进程身份锁异常".into())))?.get(&pid).cloned()
            .ok_or_else(|| anyhow!(UnverifiedStop(format!("PID {pid} 缺少 spawn 原始身份，拒绝仅凭 PID 终止"))))?;
        stop_process_handle_checked(identity.handle.as_handle())
    }
    #[cfg(unix)] { process_group::stop_registered(pid) }
    #[cfg(not(any(windows, unix)))] { let _ = pid; Err(anyhow!(UnverifiedStop("当前平台无法确证原生进程树停止，保留原执行记录".into()))) }
}

#[cfg(any(windows, test))]
fn validate_process_edge(expected_parent: u32, actual_parent: u32, parent_created: u64, parent_exited: Option<u64>, child_created: u64) -> Result<()> {
    if expected_parent != actual_parent || child_created < parent_created || parent_exited.is_some_and(|exit| child_created > exit) {
        return Err(anyhow!("后代进程父代或创建时间不匹配，拒绝终止可能复用的 PID"));
    }
    Ok(())
}

#[cfg(all(test, windows))]
static TEST_DENIED_STOPS: std::sync::LazyLock<Mutex<HashSet<u32>>> = std::sync::LazyLock::new(|| Mutex::new(HashSet::new()));
#[cfg(all(test, windows))]
pub(crate) struct TestStopFailureGuard(u32);
#[cfg(all(test, windows))]
impl TestStopFailureGuard {
    pub(crate) fn new(pid: u32) -> Self { TEST_DENIED_STOPS.lock().unwrap().insert(pid); Self(pid) }
}
#[cfg(all(test, windows))]
impl Drop for TestStopFailureGuard { fn drop(&mut self) { TEST_DENIED_STOPS.lock().unwrap().remove(&self.0); } }

#[cfg(windows)]
pub(crate) fn stop_process_handle_checked(root: std::os::windows::io::BorrowedHandle<'_>) -> Result<()> {
    #[cfg(test)] {
        use std::os::windows::io::AsRawHandle;
        let pid = unsafe { windows_sys::Win32::System::Threading::GetProcessId(root.as_raw_handle()) };
        if TEST_DENIED_STOPS.lock().unwrap().contains(&pid) {
            if let Ok(identities) = PROCESS_IDENTITIES.lock() { if let Some(identity) = identities.get(&pid) { identity.cleanup_failed.store(true, Ordering::Release); } }
            return Err(anyhow!(UnverifiedStop(format!("fixture refused stop for PID {pid}"))));
        }
    }
    let result = stop_process_handle_inner(root);
    if result.is_err() {
        use std::os::windows::io::AsRawHandle;
        let pid = unsafe { windows_sys::Win32::System::Threading::GetProcessId(root.as_raw_handle()) };
        if let Ok(identities) = PROCESS_IDENTITIES.lock() { if let Some(identity) = identities.get(&pid) { identity.cleanup_failed.store(true, Ordering::Release); } }
    }
    result.map_err(|error| anyhow!(UnverifiedStop(error.to_string())))
}

#[cfg(windows)]
fn stop_process_handle_inner(root: std::os::windows::io::BorrowedHandle<'_>) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    let pid=unsafe { windows_sys::Win32::System::Threading::GetProcessId(root.as_raw_handle()) };
    if pid==0||pid==std::process::id(){return Err(anyhow!("拒绝终止应用自身或无效进程"));}
    let identity=PROCESS_IDENTITIES.lock().map_err(|_|anyhow!("进程身份锁异常"))?.get(&pid).cloned().ok_or_else(||anyhow!("缺少spawn原始身份，拒绝仅凭PID停止"))?;
    let job=identity.job.as_ref().ok_or_else(||anyhow!("该进程未在执行前绑定Job，不能宣称完整进程树已停止"))?;
    job.stop()?;
    identity.verified.store(true,Ordering::Release);
    let mut identities=PROCESS_IDENTITIES.lock().map_err(|_|anyhow!("进程身份锁异常"))?;
    if identities.get(&pid).is_some_and(|current|Arc::ptr_eq(current,&identity)){identities.remove(&pid);}
    Ok(())
}

pub(crate) fn terminate_acp_process(process_id: &AtomicU32) {
    if let Err(error) = terminate_acp_process_checked(process_id) {
        eprintln!("[crewvela] 停止未确认，保留PID及spawn身份：{error:#}");
    }
}

fn is_read_only_crewvela_mcp_call(update: &Value) -> bool {
    const READ_ONLY_TOOLS: &[&str] = &[
        "cc_task_read",
        "cc_thread_read",
        "cc_approval_read",
        "cc_artifact_inspect",
        "cc_knowledge_search",
        "cc_memory_search",
    ];
    update.pointer("/rawInput/server").and_then(Value::as_str) == Some("crewvela")
        && update
            .pointer("/rawInput/tool")
            .and_then(Value::as_str)
            .is_some_and(|tool| READ_ONLY_TOOLS.contains(&tool))
}

fn should_allow_permission(
    request: &Value,
    auto_approve_permissions: bool,
    read_only_crewvela_mcp_calls: &HashSet<String>,
) -> bool {
    auto_approve_permissions
        || request
            .pointer("/params/toolCall/toolCallId")
            .and_then(Value::as_str)
            .is_some_and(|id| read_only_crewvela_mcp_calls.contains(id))
}

#[derive(Clone)]
pub(crate) struct AcpCancelHandle {
    stdin: Arc<Mutex<ChildStdin>>,
    session_id: String,
}

/// Uses the connection's sole reader to route replies while prompt/permission
/// processing continues. Control writes never wait on the connection mutex.
#[derive(Clone)]
pub(crate) struct CodexGoalHandle {
    stdin: Arc<Mutex<ChildStdin>>,
    session_id: String,
    replies: GoalReplies,
}

impl CodexGoalHandle {
    pub(crate) fn submit(&self, action: &str, objective: Option<&str>) -> Result<CodexGoalReply> {
        if !matches!(action, "set" | "pause" | "resume" | "clear") { return Err(anyhow!("不支持的 Codex 目标操作")); }
        let id = format!("crewvela-goal-{}", uuid::Uuid::new_v4());
        let (sender, receiver) = mpsc::sync_channel(1);
        self.replies.lock().map_err(|_| anyhow!("Codex 目标响应锁已损坏"))?.insert(id.clone(), sender);
        let mut params = json!({ "sessionId": self.session_id, "action": action });
        if let Some(objective) = objective { params["objective"] = json!(objective); }
        let reply = CodexGoalReply { id: id.clone(), receiver, replies: Arc::clone(&self.replies) };
        write_to_stdin(&self.stdin, json!({"jsonrpc":"2.0","id":id,"method":CODEX_GOAL_METHOD,"params":params}))?;
        Ok(reply)
    }
}

pub(crate) struct CodexGoalReply {
    id: String,
    receiver: mpsc::Receiver<Value>,
    replies: GoalReplies,
}

impl CodexGoalReply {
    pub(crate) fn wait(self, timeout: Duration) -> Result<()> {
        self.receiver.recv_timeout(timeout).map_err(|error| anyhow!("Codex 目标请求未完成：{error}")).and_then(|response| {
            if let Some(error) = response.get("error") { Err(anyhow!("Codex 目标操作失败：{error}")) } else { Ok(()) }
        })
    }
}

impl Drop for CodexGoalReply {
    fn drop(&mut self) { if let Ok(mut replies) = self.replies.lock() { replies.remove(&self.id); } }
}

impl AcpCancelHandle {
    pub(crate) fn cancel(&self) -> Result<()> {
        write_to_stdin(&self.stdin, cancel_notification(&self.session_id))
    }
}

fn cancel_notification(session_id: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "method": "session/cancel",
        "params": { "sessionId": session_id }
    })
}

fn write_to_stdin(stdin: &Arc<Mutex<ChildStdin>>, value: Value) -> Result<()> {
    let mut stdin = stdin.lock().map_err(|_| anyhow!("ACP stdin 锁已损坏"))?;
    serde_json::to_writer(&mut *stdin, &value).context("写入 ACP 请求失败")?;
    stdin.write_all(b"\n").context("写入 ACP 换行失败")?;
    stdin.flush().context("刷新 ACP 请求失败")
}

#[derive(Clone, Default)]
pub struct AcpManager {
    inner: Arc<Mutex<Option<AcpConnection>>>,
    process_id: Arc<AtomicU32>,
    cancel_handle: Arc<Mutex<Option<AcpCancelHandle>>>,
}

impl AcpManager {
    pub fn start(&self, cwd: &str, resume_session_id: Option<&str>) -> Result<AcpStatus> {
        let mut state = self.inner.lock().map_err(|_| anyhow!("ACP 状态锁已损坏"))?;
        if let Some(connection) = state.as_mut() {
            if connection.cwd == cwd {
                return connection.status(true);
            }
        }
        let connection =
            AcpConnection::spawn(cwd, resume_session_id, Arc::clone(&self.process_id))?;
        let cancel_handle = AcpCancelHandle {
            stdin: Arc::clone(&connection.stdin),
            session_id: connection.session_id.clone(),
        };
        *state = Some(connection);
        *self
            .cancel_handle
            .lock()
            .map_err(|_| anyhow!("ACP 取消状态锁已损坏"))? = Some(cancel_handle);
        let resumed = state
            .as_ref()
            .is_some_and(|connection| connection.last_start_resumed);
        state.as_mut().context("ACP 会话启动失败")?.status(resumed)
    }

    pub fn new_session(&self, cwd: &str) -> Result<AcpStatus> {
        let mut state = self.inner.lock().map_err(|_| anyhow!("ACP 状态锁已损坏"))?;
        *state = None;
        *self
            .cancel_handle
            .lock()
            .map_err(|_| anyhow!("ACP 取消状态锁已损坏"))? = None;
        let connection = AcpConnection::spawn(cwd, None, Arc::clone(&self.process_id))?;
        let cancel_handle = AcpCancelHandle {
            stdin: Arc::clone(&connection.stdin),
            session_id: connection.session_id.clone(),
        };
        *state = Some(connection);
        *self
            .cancel_handle
            .lock()
            .map_err(|_| anyhow!("ACP 取消状态锁已损坏"))? = Some(cancel_handle);
        state.as_mut().context("ACP 会话启动失败")?.status(false)
    }

    pub fn prompt(&self, cwd: &str, message: &str) -> Result<AcpPromptResult> {
        let mut state = self.inner.lock().map_err(|_| anyhow!("ACP 状态锁已损坏"))?;
        let connection = state
            .as_mut()
            .filter(|connection| connection.cwd == cwd)
            .ok_or_else(|| anyhow!("ACP 会话尚未启动或工作目录已变化"))?;
        connection.prompt(message)
    }

    pub fn cancel_prompt(&self) -> Result<bool> {
        let handle = self
            .cancel_handle
            .lock()
            .map_err(|_| anyhow!("ACP 取消状态锁已损坏"))?
            .clone();
        let Some(handle) = handle else {
            return Ok(false);
        };
        handle.cancel()?;
        Ok(true)
    }

    /// 多智能体控制面连接是否存活（keep-awake 活跃判定用）。
    pub fn is_running(&self) -> bool {
        self.inner
            .lock()
            .map(|state| state.is_some())
            .unwrap_or(false)
    }

    pub fn stop(&self) -> Result<()> {
        terminate_acp_process_checked(&self.process_id)?;
        let mut state = self.inner.lock().map_err(|_| anyhow!("ACP状态锁异常"))?;
        if let Some(connection) = state.as_mut() { connection.stop_checked()?; }
        *state = None;
        *self.cancel_handle.lock().map_err(|_| anyhow!("ACP取消状态锁异常"))? = None;
        Ok(())
    }
}

fn research_session_params(cwd: &str, servers: &[Value], research: Option<ManagedResearchPolicy>) -> Value {
    let Some(policy) = research else { return json!({"cwd":cwd,"mcpServers":servers}); };
    json!({"cwd":cwd,"mcpServers":if policy.tools_enabled { servers.to_vec() } else { Vec::new() },
        "_meta":{"claudeCode":{"options":{
            "tools":[], "settingSources":[], "strictMcpConfig":true,
            "settings":{"disableAllHooks":true},
            "disallowedTools":["Bash","Edit","Write","NotebookEdit","Agent","Task","Skill","WebSearch","WebFetch","Read","Glob","Grep"],
            "extraArgs":{"disable-slash-commands":""}
        }}}})
}

fn managed_session_params(adapter: ManagedAcpKind,cwd:&str,servers:&[Value],research:Option<ManagedResearchPolicy>,native_group:bool)->Value {
    let mut params=research_session_params(cwd,servers,research);
    if adapter==ManagedAcpKind::ClaudeCode && native_group && research.is_none() {
        // Native SDK session grants: preserve user/project/local ask+deny rules,
        // account/model/settings and the full tool set. Never bypass permissions.
        params["_meta"]=json!({"claudeCode":{"options":{"allowedTools":["WebSearch","WebFetch"]}}});
    }
    params
}

fn research_provider_env(settings: &Value) -> Result<HashMap<String, String>> {
    const KEYS: &[&str] = &["ANTHROPIC_AUTH_TOKEN", "ANTHROPIC_API_KEY", "ANTHROPIC_BASE_URL", "ANTHROPIC_MODEL",
        "ANTHROPIC_REASONING_MODEL", "ANTHROPIC_SMALL_FAST_MODEL", "CLAUDE_CODE_EFFORT_LEVEL",
        "CLAUDE_CODE_USE_BEDROCK", "CLAUDE_CODE_USE_VERTEX", "CLAUDE_CODE_USE_FOUNDRY",
        "AWS_ACCESS_KEY_ID", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN", "AWS_REGION", "AWS_DEFAULT_REGION", "AWS_PROFILE",
        "GOOGLE_APPLICATION_CREDENTIALS", "CLOUD_ML_REGION", "ANTHROPIC_VERTEX_PROJECT_ID",
        "ANTHROPIC_FOUNDRY_API_KEY", "ANTHROPIC_FOUNDRY_RESOURCE", "ANTHROPIC_FOUNDRY_BASE_URL"];
    let mut result = HashMap::new();
    if let Some(environment) = settings.get("env").and_then(Value::as_object) {
        for (key, value) in environment {
            let model_alias = key.starts_with("ANTHROPIC_DEFAULT_") && (key.ends_with("_MODEL") || key.ends_with("_MODEL_NAME"))
                && key.chars().all(|ch| ch.is_ascii_uppercase() || ch == '_');
            if KEYS.contains(&key.as_str()) || model_alias {
                result.insert(key.clone(), value.as_str().ok_or_else(|| anyhow!("Claude 认证或模型环境配置必须为字符串：{key}"))?.to_string());
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod conversation_policy_tests {
    use super::*;
    #[test]
    fn native_group_approval_selects_allow_once_and_never_persists_permissions() {
        let root=std::env::temp_dir().join(format!("crewvela-once-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let script=root.join("adapter.cjs");
        std::fs::write(&script,r#"
const rl=require('node:readline').createInterface({input:process.stdin});let promptId;
const send=(id,result)=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
rl.on('line',line=>{const f=JSON.parse(line);
 if(f.result&&f.id==='permission'){
  process.stdout.write(JSON.stringify({jsonrpc:'2.0',method:'session/update',params:{sessionId:'fixture',update:{sessionUpdate:'agent_message_chunk',content:{type:'text',text:JSON.stringify(f.result.outcome)}}}})+'\n');send(promptId,{stopReason:'end_turn'});return;
 }
 if(f.method==='initialize')send(f.id,{protocolVersion:1,agentCapabilities:{}});
 else if(f.method==='session/new')send(f.id,{sessionId:'fixture'});
 else if(f.method==='session/prompt'){
  promptId=f.id;
  const options=[{optionId:'persistent',kind:'allow_always'},...(f.params.prompt[0].text==='once'?[{optionId:'single',kind:'allow_once'}]:[])];
  process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:'permission',method:'session/request_permission',params:{sessionId:'fixture',toolCall:{toolCallId:'tool-1',title:'Test command',kind:'execute'},options}})+'\n');
 }else if(f.id!==undefined)send(f.id,{});
});
"#).unwrap();
        let mut command=Command::new("node");command.arg(&script);
        let mut connection=AcpConnection::spawn_for_with_cli_identity(ManagedAcpKind::ClaudeCode,root.to_str().unwrap(),None,vec![],&[],false,false,Arc::new(AtomicU32::new(0)),None,Some(command),None).unwrap();
        connection.permission_handler=Some(Box::new(|_,_|Ok(true)));
        connection.single_use_permissions=true;
        let once=connection.prompt("once").unwrap();
        let no_once=connection.prompt("persistent-only").unwrap();
        connection.stop_checked().unwrap();drop(connection);std::fs::remove_dir_all(root).unwrap();
        assert_eq!(serde_json::from_str::<Value>(&once.text).unwrap(),json!({"outcome":"selected","optionId":"single"}));
        assert_eq!(serde_json::from_str::<Value>(&no_once.text).unwrap(),json!({"outcome":"cancelled"}));
    }
    #[test]
    fn native_group_web_grants_use_sdk_options_without_changing_user_policy() {
        let params=managed_session_params(ManagedAcpKind::ClaudeCode,"cwd",&[],None,true);
        let options=&params["_meta"]["claudeCode"]["options"];
        assert_eq!(options["allowedTools"],json!(["WebSearch","WebFetch"]));
        assert_eq!(options.as_object().unwrap().len(),1,"do not replace tools, settingsSources, model or permissionMode");
        assert!(managed_session_params(ManagedAcpKind::ClaudeCode,"cwd",&[],None,false).get("_meta").is_none());
        assert!(managed_session_params(ManagedAcpKind::Codex,"cwd",&[],None,true).get("_meta").is_none());
    }
    #[test]
    fn conversation_research_has_no_native_tools_hooks_user_context_or_user_mcp() {
        let gateway = json!({"name":"crewvela","type":"http","url":"http://127.0.0.1:1234/mcp","headers":[]});
        let research = research_session_params("isolated", &[gateway.clone()], Some(ManagedResearchPolicy { tools_enabled:true }));
        let options = &research["_meta"]["claudeCode"]["options"];
        assert_eq!(options["tools"], json!([]));
        assert_eq!(options["settingSources"], json!([]));
        assert_eq!(options["strictMcpConfig"], true);
        assert_eq!(options["settings"]["disableAllHooks"], true);
        assert_eq!(research["mcpServers"], json!([gateway.clone()]));
        let reply = research_session_params("isolated", &[gateway.clone()], Some(ManagedResearchPolicy { tools_enabled:false }));
        assert_eq!(reply["mcpServers"], json!([]));
        assert_eq!(reply["_meta"]["claudeCode"]["options"]["tools"], json!([]));
        let native = research_session_params("original", &[gateway.clone()], None);
        assert_eq!(native, json!({"cwd":"original","mcpServers":[gateway]}), "native chats and engineering keep their original configuration semantics");
    }
    #[test]
    fn conversation_auth_import_preserves_provider_but_excludes_executable_configuration() {
        let env = research_provider_env(&json!({"env":{"ANTHROPIC_AUTH_TOKEN":"test-only-token", "ANTHROPIC_BASE_URL":"https://provider.example",
            "ANTHROPIC_DEFAULT_SONNET_MODEL":"chosen-model", "NODE_OPTIONS":"--require unsafe.js", "CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS":"1", "ENABLE_TOOL_SEARCH":"1"},
            "apiKeyHelper":"unsafe-command", "hooks":{"PreToolUse":["unsafe"]}, "enabledPlugins":{"unsafe":true}})).unwrap();
        assert_eq!(env.len(), 3);
        assert_eq!(env["ANTHROPIC_BASE_URL"], "https://provider.example");
        assert_eq!(env["ANTHROPIC_DEFAULT_SONNET_MODEL"], "chosen-model");
        assert!(!env.contains_key("NODE_OPTIONS"));
    }
}

pub(crate) fn execute_managed_research_acp(
    kind: ManagedAcpKind, app_session_id: &str, cwd: &str, prompt: &str,
    mcp_url: &str, token: &str, tools_enabled: bool, cancelled: &AtomicBool,
    update_handler: Option<Box<dyn FnMut(&Value) + Send>>,
) -> Result<ManagedAcpOutcome> {
    let permissions: ManagedPermissionHandler = Box::new(move |request, _| {
        let call = &request["params"]["toolCall"];
        let named = call["title"].as_str().or_else(|| call["rawInput"]["name"].as_str()).unwrap_or("");
        let name = if call["rawInput"]["server"].as_str() == Some("crewvela") { call["rawInput"]["tool"].as_str().unwrap_or("") }
            else { named.strip_prefix("mcp__crewvela__").unwrap_or("") };
        Ok(tools_enabled && matches!(name, "cc_research_read" | "cc_research_list" | "cc_research_search" | "cc_research_web_search" | "cc_research_web_fetch"))
    });
    execute_managed_acp_inner(kind, app_session_id, cwd, None, prompt, mcp_url, token,
        ManagedAcpMode::ReadOnly, cancelled, None, Some(permissions), update_handler,
        Some(ManagedResearchPolicy { tools_enabled }))
}

pub(crate) fn execute_managed_acp(
    kind: ManagedAcpKind,
    app_session_id: &str,
    cwd: &str,
    resume_session_id: Option<&str>,
    prompt: &str,
    mcp_url: &str,
    token: &str,
    mode: ManagedAcpMode,
    cancelled: &AtomicBool,
    model: Option<&str>,
    permission_handler: Option<ManagedPermissionHandler>,
    update_handler: Option<Box<dyn FnMut(&Value) + Send>>,
) -> Result<ManagedAcpOutcome> {
    execute_managed_acp_inner(kind, app_session_id, cwd, resume_session_id, prompt, mcp_url, token,
        mode, cancelled, model, permission_handler, update_handler, None)
}

fn execute_managed_acp_inner(
    kind: ManagedAcpKind,
    app_session_id: &str,
    cwd: &str,
    resume_session_id: Option<&str>,
    prompt: &str,
    mcp_url: &str,
    token: &str,
    mode: ManagedAcpMode,
    cancelled: &AtomicBool,
    model: Option<&str>,
    permission_handler: Option<ManagedPermissionHandler>,
    update_handler: Option<Box<dyn FnMut(&Value) + Send>>,
    research: Option<ManagedResearchPolicy>,
) -> Result<ManagedAcpOutcome> {
    let process_id = Arc::new(AtomicU32::new(0));
    // 保留配置中启用的 http 类型用户 server 一并交给 Agent；
    // 读取失败不阻塞：协作桥本身仍然是必需的控制面。
    let mut mcp_servers = if mcp_url.is_empty() { Vec::new() } else { vec![managed_mcp_server(mcp_url, token)] };
    if !mcp_url.is_empty() && research.is_none() { match crate::mcp::enabled_acp_servers() {
        Ok(user_servers) => mcp_servers.extend(user_servers),
        Err(error) => eprintln!("[crewvela] 读取用户 MCP 配置失败：{error:#}"),
    } }
    let mut connection = match AcpConnection::spawn_for_with_cli_identity_policy(
        kind,
        cwd,
        resume_session_id,
        mcp_servers,
        if kind == ManagedAcpKind::DeepSeekHarness {
            match mode {
                ManagedAcpMode::Inherit => &[],
                ManagedAcpMode::ReadOnly => &["read-only"],
                ManagedAcpMode::FullAccess => &["agent-full-access"],
                _ => &["workspace-write"],
            }
        } else {
            mode.candidates()
        },
        matches!(mode, ManagedAcpMode::Auto | ManagedAcpMode::FullAccess),
        true,
        process_id,
        Some(app_session_id),
        None,
        None,
        research,
    ) {
        Ok(connection) => connection,
        Err(error) if error.downcast_ref::<UnverifiedStop>().is_some() => return Err(error),
        Err(error) => return Ok(ManagedAcpOutcome::Unavailable(format!("{error:#}"))),
    };
    let outcome = (|| -> Result<ManagedAcpOutcome> {
    connection.permission_handler = permission_handler;
    connection.single_use_permissions = matches!(mode,ManagedAcpMode::Inherit);
    if let Some(handler) = update_handler { connection.set_update_handler(handler); }
    if kind == ManagedAcpKind::DeepSeekHarness {
        if let Some(model) = model.filter(|model| !model.trim().is_empty()) {
            let value = resolve_deepseek_model(&connection.config_options, model)?;
            connection.request(
                "session/set_config_option",
                json!({"sessionId": connection.session_id, "configId": "model", "value": value}),
                START_TIMEOUT,
                None,
            )?;
        }
    }
    // spawn_for 阶段（下载适配器、initialize、session/new）不感知取消，这里补一次检查
    if cancelled.load(Ordering::Acquire) {
        return Err(anyhow!("ACP Agent 任务已取消"));
    }
    let result = connection.prompt_cancellable(prompt, Some(cancelled))?;
    if result.cancelled || cancelled.load(Ordering::Acquire) {
        return Err(anyhow!("ACP Agent 任务已取消"));
    }
    if result.text.trim().is_empty() {
        return Err(anyhow!("{} ACP Agent 未返回可用结果", kind.label()));
    }
    if kind == ManagedAcpKind::DeepSeekHarness {
        connection.request(
            "session/close",
            json!({"sessionId": connection.session_id}),
            START_TIMEOUT,
            None,
        )?;
    }
    Ok(ManagedAcpOutcome::Completed(ManagedAcpResult {
        text: result.text,
        session_id: connection.session_id.clone(),
        agent_name: connection.agent_name.clone(),
        agent_version: connection.agent_version.clone(),
    }))
    })();
    finish_managed_connection(connection, outcome)
}

fn finish_managed_connection(mut connection: AcpConnection, outcome: Result<ManagedAcpOutcome>) -> Result<ManagedAcpOutcome> {
    if let Err(error) = connection.stop_checked() {
        quarantine_acp_connection(connection);
        return Err(error);
    }
    outcome
}

fn resolve_deepseek_model(config_options: &[Value], model: &str) -> Result<String> {
    let mut matches = HashSet::new();
    let options = config_options
        .iter()
        .find(|option| option.get("id").and_then(Value::as_str) == Some("model"))
        .and_then(|option| option.get("options"))
        .and_then(Value::as_array)
        .context("DeepSeek Harness 未公布 model 选择项")?;
    for group in options {
        let leaves = group
            .get("options")
            .and_then(Value::as_array)
            .map(|options| options.as_slice())
            .unwrap_or(std::slice::from_ref(group));
        for option in leaves {
            let Some(value) = option.get("value").and_then(Value::as_str) else {
                continue;
            };
            let route: Option<Vec<String>> = serde_json::from_str(value).ok();
            let route_match = route.as_ref().is_some_and(|parts| {
                parts.len() == 2
                    && (parts[1] == model || format!("{}/{}", parts[0], parts[1]) == model)
            });
            if value == model
                || option.get("name").and_then(Value::as_str) == Some(model)
                || route_match
            {
                matches.insert(value.to_string());
            }
        }
    }
    if matches.len() != 1 {
        return Err(anyhow!("DeepSeek Harness 模型选择无效或有歧义：{model}；请使用已配置的 provider/model 或完整 ACP value"));
    }
    Ok(matches.into_iter().next().unwrap())
}

fn configure_session_cli_identity(command: &mut Command, app_session_id: &str) -> Result<()> {
    // Per-child environment avoids leaking one Agent's sender identity into another run.
    command.env("CREWVELA_SESSION_ID", app_session_id).env(
        "CREWVELA_CLI",
        std::env::current_exe().context("无法确定 CrewVela 可执行文件")?,
    );
    Ok(())
}

fn managed_mcp_server(url: &str, token: &str) -> Value {
    json!({
        "type": "http",
        "name": "crewvela",
        "url": url,
        "headers": [{
            "name": "Authorization",
            "value": format!("Bearer {token}")
        }]
    })
}
fn configure_codex_acp_cli_identity(command: &mut Command, app_session_id: &str) -> Result<()> {
    let existing = command
        .get_envs()
        .find(|(key, _)| *key == "CODEX_CONFIG")
        .and_then(|(_, value)| value.map(|value| value.to_string_lossy().into_owned()))
        .or_else(|| std::env::var("CODEX_CONFIG").ok());
    let mut config = match existing {
        Some(value) => serde_json::from_str::<Value>(&value)
            .context("Codex ACP 的 CODEX_CONFIG 不是有效 JSON")?,
        None => serde_json::json!({}),
    };
    let object = config
        .as_object_mut()
        .context("Codex ACP 的 CODEX_CONFIG 必须是 JSON 对象")?;
    object.insert(
        "shell_environment_policy.set.CREWVELA_SESSION_ID".into(),
        Value::String(app_session_id.into()),
    );
    object.insert(
        "shell_environment_policy.set.CREWVELA_CLI".into(),
        Value::String(std::env::current_exe()?.display().to_string()),
    );
    command.env("CODEX_CONFIG", serde_json::to_string(&config)?);
    Ok(())
}

pub(crate) fn canonical_harness_directory(path: &str) -> Result<String> {
    let canonical = Path::new(path)
        .canonicalize()
        .with_context(|| format!("无法解析工作目录: {path}"))?;
    if !canonical.is_dir() {
        return Err(anyhow!("请选择存在的工作目录后再启动 DeepSeek Harness"));
    }
    normalize_harness_path(&canonical)
}

pub(crate) fn normalize_harness_path(canonical: &Path) -> Result<String> {
    let raw = canonical.to_string_lossy();
    #[cfg(target_os = "windows")]
    let external = harness_windows_path(&raw);
    #[cfg(not(target_os = "windows"))]
    let external = raw.into_owned();
    #[cfg(target_os = "windows")]
    if external.starts_with(r"\\?\") {
        return Err(anyhow!("DeepSeek Harness 不支持此 Windows 设备路径，请选择盘符或 UNC 工作目录"));
    }
    // Removing the prefix must not resolve a trailing-dot or device name to a
    // different directory under normal Win32 path rules.
    if Path::new(&external).canonicalize()? != canonical {
        return Err(anyhow!("工作目录无法安全转换为 DeepSeek Harness 支持的路径"));
    }
    Ok(external)
}

#[cfg(any(target_os = "windows", test))]
fn harness_windows_path(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else if let Some(rest) = path.strip_prefix(r"\\?\") {
        // Only DOS drive paths may lose this prefix. Device paths remain intact.
        if rest.as_bytes().get(1) == Some(&b':')
            && rest.as_bytes().first().is_some_and(u8::is_ascii_alphabetic)
            && rest.as_bytes().get(2) == Some(&b'\\')
        {
            rest.to_string()
        } else {
            path.to_string()
        }
    } else {
        path.to_string()
    }
}

fn adapter_command(adapter: ManagedAcpKind) -> Result<Command> {
    if adapter == ManagedAcpKind::Codex {
        return codex_adapter_command();
    }
    if adapter == ManagedAcpKind::DeepSeekHarness {
        let mut command = deepseek_command()?;
        command.args(["--profile", "acp"]);
        return Ok(command);
    }
    // kimi / opencode CLI 原生自带 ACP server（非 npm 适配器），不走 npx 兜底。
    if adapter == ManagedAcpKind::Kimi {
        return kimi_acp_command();
    }
    if adapter == ManagedAcpKind::OpenCode {
        return opencode_acp_command();
    }
    let binary = adapter.binary();
    for root in crate::runtime_dependencies::node_modules_candidates() {
        let local = root.join(".bin").join(if cfg!(target_os = "windows") {
            format!("{binary}.cmd")
        } else { binary.to_string() });
        if local.is_file() { return Ok(command_for_script(&local)); }
    }
    if let Ok(global) = which::which(binary) {
        return Ok(command_for_script(&global));
    }
    let npx = which::which(if cfg!(target_os = "windows") {
        "npx.cmd"
    } else {
        "npx"
    })
    .with_context(|| format!("未找到 {binary}，也未找到 npx；请先安装 Node.js 22+"))?;
    let mut command = npx_command(&npx)?;
    command.arg("--yes").arg(adapter.package());
    Ok(command)
}

fn materialize_codex_wrapper_at(directory: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(directory)?;
    let path = directory.join("codex-acp-wrapper-1.11.0.mjs");
    let source = include_str!("../../tools/codex-acp-wrapper.mjs");
    if std::fs::read_to_string(&path).ok().as_deref() != Some(source) {
        std::fs::write(&path, source)?;
    }
    Ok(path)
}

fn codex_adapter_command() -> Result<Command> {
    let node = which::which("node").context("Codex ACP 需要 Node.js")?;
    let wrapper = materialize_codex_wrapper_at(&crate::brand::config_dir()?.join("adapters"))?;
    let entry = crate::runtime_dependencies::node_modules_candidates().into_iter()
        .map(|root| root.join("@agentclientprotocol/codex-acp/dist/index.js"))
        .find(|entry| {
            entry.is_file() && entry.parent().and_then(Path::parent)
                .and_then(|root| std::fs::read_to_string(root.join("package.json")).ok())
                .and_then(|source| serde_json::from_str::<Value>(&source).ok())
                .is_some_and(|package| package.get("version").and_then(Value::as_str) == Some("1.11.0"))
        });
    let mut command = if let Some(entry) = entry {
        let mut command = Command::new(&node);
        command.arg(&wrapper).arg(entry);
        command
    } else {
        let npx = which::which(if cfg!(windows) { "npx.cmd" } else { "npx" })
            .context("未找到 Codex ACP 适配器或 npx；请安装 Node.js/npm")?;
        codex_npx_command(&npx, &node, &wrapper)?
    };
    command.env("CREWVELA_CODEX_ACP_BRIDGE", "1");
    Ok(command)
}

fn codex_npx_command(npx: &Path, node: &Path, wrapper: &Path) -> Result<Command> {
    let mut command = npx_command(npx)?;
    // npm exec joins positional arguments into shell text. An absolute Node
    // path under Program Files gets split, and wrapper paths can contain shell
    // metacharacters. Keep shell text constant; pass the path only via env.
    command.args(["--yes", "--package", CODEX_ACP_ADAPTER_PACKAGE, "--call", CODEX_ACP_NPX_BOOTSTRAP]);
    command.env("CREWVELA_CODEX_ACP_WRAPPER", wrapper);
    let node_dir = node.parent().context("Node.js 路径没有父目录")?;
    let mut paths = vec![node_dir.to_path_buf()];
    if let Some(inherited) = std::env::var_os("PATH") { paths.extend(std::env::split_paths(&inherited)); }
    command.env("PATH", std::env::join_paths(paths).context("无法构造 Codex ACP 的 Node.js 查找路径")?);
    Ok(command)
}

fn npx_command(path: &Path) -> Result<Command> {
    #[cfg(target_os = "windows")]
    if path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("cmd")) {
        // npx.cmd 通常位于 Program Files；经 cmd /s /c 启动会截断带空格的路径。
        // 直接运行 npm 的同一 JS 入口，参数继续交给 Command 转义。
        let directory = path.parent().context("npx 路径没有父目录")?;
        let entry = directory.join("node_modules/npm/bin/npx-cli.js");
        if !entry.is_file() {
            return Err(anyhow!("未找到 npx 的 npm 入口 {}；请修复或重新安装 Node.js/npm", entry.display()));
        }
        let bundled_node = directory.join("node.exe");
        let node = if bundled_node.is_file() { bundled_node } else {
            which::which("node").context("运行 npx 需要 Node.js；请修复 Node.js/npm 安装")?
        };
        let mut command = Command::new(node);
        command.arg(entry);
        return Ok(command);
    }
    Ok(Command::new(path))
}

pub(crate) fn deepseek_executable() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("CREWVELA_DSH_ROOT").filter(|root| !root.is_empty()) {
        let root = PathBuf::from(root);
        let entry = root.join("apps/cli/src/bin.ts");
        if !root.is_absolute()
            || !entry.is_file()
            || !root.join("node_modules/tsx/dist/esm/index.mjs").is_file()
        {
            return Err(anyhow!(
                "CREWVELA_DSH_ROOT 必须指向已安装依赖的 DeepSeek Harness 源码根目录"
            ));
        }
        which::which("node").context("DeepSeek Harness 源码启动需要 Node.js")?;
        return Ok(entry);
    }
    which::which("dsh").context("未找到 dsh；请安装 DeepSeek Harness CLI，或配置 CREWVELA_DSH_ROOT")
}

/// kimi CLI 原生 ACP server 启动命令：`kimi acp`（stdio JSON-RPC，无需适配器）。
pub(crate) fn kimi_acp_command() -> Result<Command> {
    let executable = crate::multi_agent::resolve_runtime_executable_checked(crate::multi_agent::AgentRuntimeKind::KimiCode)
        .map_err(anyhow::Error::msg)?;
    let mut command = command_for_script(&executable);
    command.arg("acp");
    Ok(command)
}

pub(crate) fn opencode_acp_command() -> Result<Command> {
    let executable = crate::multi_agent::resolve_runtime_executable_checked(crate::multi_agent::AgentRuntimeKind::OpenCode)
        .map_err(anyhow::Error::msg)?;
    let mut command = command_for_script(&executable);
    command.args(["acp", "--print-logs", "--log-level", "ERROR"]);
    Ok(command)
}

fn opencode_terminal_prompt_failure(log: &str, session_id: &str) -> Option<&'static str> {
    if session_id.is_empty() { return None; }
    let session_field = format!("session.id={session_id}");
    for line in log.lines().rev() {
        let Some((header, error)) = line.split_once(" error.error=") else { continue; };
        if !header.starts_with("timestamp=") || !header.contains("message=\"stream error\"") { continue; }
        let fields = header.split_ascii_whitespace().collect::<Vec<_>>();
        if !fields.contains(&"level=ERROR") || !fields.contains(&"small=false") || !fields.contains(&session_field.as_str()) { continue; }
        let error = error.to_ascii_lowercase();
        // Only unambiguous terminal failures. Generic errors/429s can be retried
        // by OpenCode and must not abort an otherwise healthy streaming turn.
        if error.contains("free usage limit reached") {
            return Some("OpenCode 当前模型的免费额度已用尽，本轮未生成回复。可选择其他已有的可用模型，或等待额度恢复后重试。");
        }
        if ["invalid api key", "invalid_api_key", "incorrect api key", "authentication failed", "unauthorized"].iter().any(|value| error.contains(value)) {
            return Some("OpenCode 当前模型认证失败，请检查对应服务的登录或密钥配置后重试。");
        }
        if ["providermodelnotfounderror", "model not found", "model does not exist"].iter().any(|value| error.contains(value)) {
            return Some("OpenCode 当前模型不存在或不可用，请检查模型配置并选择你可用的模型。");
        }
        if ["insufficient_quota", "insufficient balance", "credit balance is too low"].iter().any(|value| error.contains(value)) {
            return Some("OpenCode 当前模型的服务额度或余额不足，请检查对应服务后重试。");
        }
    }
    None
}

pub(crate) fn deepseek_command() -> Result<Command> {
    let executable = deepseek_executable()?;
    if executable.extension().is_some_and(|ext| ext == "ts") {
        let root = executable
            .parent()
            .and_then(Path::parent)
            .and_then(Path::parent)
            .and_then(Path::parent)
            .context("DeepSeek Harness 源码入口路径无效")?;
        let mut command = Command::new(which::which("node").context("源码启动需要 Node.js")?);
        command
            .arg("--import")
            .arg(root.join("node_modules/tsx/dist/esm/index.mjs"))
            .arg(executable);
        Ok(command)
    } else {
        Ok(command_for_script(&executable))
    }
}
fn command_for_script(path: &Path) -> Command {
    if cfg!(target_os = "windows")
        && path.extension().is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
    {
        let mut command = Command::new("cmd.exe");
        command.arg("/d").arg("/s").arg("/c").arg(path);
        command
    } else {
        Command::new(path)
    }
}

fn capture_stderr(stderr: impl std::io::Read + Send + 'static) -> Arc<Mutex<String>> {
    let captured = Arc::new(Mutex::new(String::new()));
    let target = Arc::clone(&captured);
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Ok(mut value) = target.lock() {
                value.push_str(&line);
                value.push('\n');
                if value.len() > 12_000 {
                    let mut drain_to = value.len().saturating_sub(8_000);
                    while !value.is_char_boundary(drain_to) { drain_to += 1; }
                    value.drain(..drain_to);
                }
            }
        }
    });
    captured
}

pub(crate) fn parse_commands(update: &Value) -> Vec<AcpCommand> {
    let mut commands = update
        .get("availableCommands")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|command| {
            let name = command.get("name")?.as_str()?.trim();
            if name.is_empty() {
                return None;
            }
            Some(AcpCommand {
                name: name.to_string(),
                description: command
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                input_hint: command
                    .pointer("/input/hint")
                    .and_then(Value::as_str)
                    .map(str::to_string),
            })
        })
        .collect::<Vec<_>>();
    // 上游可能同时报告同名命令（用户技能与内置撞名，如 code-review）；
    // 保留首个出现，菜单端再按目录分类。重复项会导致前端列表 key 冲突。
    let mut seen = std::collections::HashSet::new();
    commands.retain(|command| seen.insert(command.name.clone()));
    commands
}

fn choose_permission_option(request: &Value, allow: bool) -> Option<String> {
    let options = request.pointer("/params/options")?.as_array()?;
    let preferred: &[&str] = if allow {
        &["allow_always", "allow_once"]
    } else {
        &["reject_once", "reject_always"]
    };
    for kind in preferred {
        if let Some(option) = options.iter().find(|option| {
            option.get("kind").and_then(Value::as_str) == Some(*kind)
                && option.get("optionId").and_then(Value::as_str).is_some()
        }) {
            return option
                .get("optionId")
                .and_then(Value::as_str)
                .map(str::to_string);
        }
    }
    if !allow {
        // 拒绝时绝不回退到任意（可能是 allow_*）选项，避免只读/默认模式下误批
        return None;
    }
    options
        .first()
        .and_then(|option| option.get("optionId"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    #[test]
    fn codex_acp_terminal_failure_is_not_a_successful_reply() {
        let root = std::env::temp_dir().join(format!("acp-codex-failure-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let script = root.join("adapter.cjs");
        // Reproduce codex-acp's legacy text fallback and negotiated terminal result.
        std::fs::write(&script, r#"
const rl=require('readline').createInterface({input:process.stdin});
let typed=false;
const send=(id,result)=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
const failure={id:'turn:error',revision:1,severity:'error',category:'invalid_request',title:JSON.stringify({type:'error',status:400,error:{type:'invalid_request_error',message:"The 'gpt-6.1-terra' model is not supported when using Codex with a ChatGPT account.",code:'model_not_found'}}),actions:[]};
const meta=f=>({jetbrains:{air:{version:1,sessionFailure:f}}});
rl.on('line',line=>{
 const f=JSON.parse(line); if(f.id===undefined)return;
 if(f.method==='initialize') {typed=f.params.clientCapabilities?._meta?.jetbrains?.air?.capabilities?.includes('sessionFailure');send(f.id,{protocolVersion:1,agentCapabilities:{}});}
 else if(f.method==='session/new') send(f.id,{sessionId:'fixture',modes:{currentModeId:'plan',availableModes:[{id:'plan',name:'Plan'}]}});
 else if(f.method==='session/prompt') {
  const input=f.params.prompt[0].text;
  const failed=input==='failure', quoted=input==='quoted', warning=input==='warning';
  const text=failed?(typed?'':'Warning: Model metadata unavailable\n\n'+failure.title):(quoted?'Example error JSON: '+failure.title:'Real reply');
  if(text)process.stdout.write(JSON.stringify({jsonrpc:'2.0',method:'session/update',params:{sessionId:'fixture',update:{sessionUpdate:'agent_message_chunk',content:{type:'text',text}}}})+'\n');
  send(f.id,{stopReason:'end_turn',...(typed&&failed?{_meta:meta(failure)}:warning?{_meta:meta({...failure,severity:'warning'})}:{})});
 } else send(f.id,{});
});
setInterval(()=>{},1000);
"#).unwrap();
        let mut command = Command::new("node");
        command.arg(&script);
        let mut connection = AcpConnection::spawn_for_with_cli_identity(
            ManagedAcpKind::Codex, root.to_str().unwrap(), None, Vec::new(), &["plan"],
            false, false, Arc::new(AtomicU32::new(0)), None, Some(command), None,
        ).unwrap();
        let failure = connection.prompt("failure");
        // Always reap the owned process, even when checking the pre-fix behavior.
        let normal = connection.prompt("normal").unwrap();
        let quoted = connection.prompt("quoted").unwrap();
        let warning = connection.prompt("warning").unwrap();
        connection.stop_checked().unwrap();
        drop(connection);
        std::fs::remove_dir_all(root).unwrap();
        let error = failure.err().expect("provider HTTP 400 must not become a successful reply");
        assert!(error.to_string().contains("gpt-6.1-terra"), "{error}");
        assert!(error.to_string().contains("not supported"), "{error}");
        assert!(error.to_string().contains("model_not_found"), "{error}");
        assert_eq!(normal.text, "Real reply");
        assert!(quoted.text.starts_with("Example error JSON:"));
        assert_eq!(warning.text, "Real reply");
    }

    #[test]
    #[ignore = "需要本机 OpenCode 和网络，使用当前账号验证免费模型回复或额度错误反馈，不使用工具"]
    fn real_opencode_free_model_reports_reply_or_quota_promptly() {
        let root = std::env::temp_dir().join(format!("crewvela-opencode-diagnostic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut connection = super::AcpConnection::spawn_native_opencode_chat(
            root.to_str().unwrap(), None, Vec::new(), Box::new(|_, _| Ok(false)),
            std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0)), None,
        ).unwrap();
        connection.set_config_option("model", "opencode/glm-5.3-flash-free").unwrap();
        let done = std::sync::atomic::AtomicBool::new(false);
        let cancelled = std::sync::atomic::AtomicBool::new(false);
        let started = std::time::Instant::now();
        let result = std::thread::scope(|scope| {
            scope.spawn(|| {
                while !done.load(std::sync::atomic::Ordering::Acquire) {
                    if started.elapsed() > std::time::Duration::from_secs(25) {
                        cancelled.store(true, std::sync::atomic::Ordering::Release);
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            });
            let result = connection.prompt_content_interactive(vec![serde_json::json!({"type":"text","text":"Reply with exactly OK. Do not use tools."})], &cancelled);
            done.store(true, std::sync::atomic::Ordering::Release);
            result
        });
        connection.stop_checked().unwrap();
        drop(connection);
        std::fs::remove_dir_all(root).unwrap();
        match result {
            Ok(reply) => { assert!(!reply.cancelled && !reply.text.trim().is_empty()); println!("OPENCODE_LIVE_RESULT reply_present=true"); }
            Err(error) => { assert!(error.to_string().contains("免费额度"), "unexpected diagnostic: {error}"); println!("OPENCODE_LIVE_RESULT quota_error_visible=true"); }
        }
        assert!(started.elapsed() < std::time::Duration::from_secs(25));
    }

    #[test]
    fn opencode_terminal_diagnostics_are_scoped_redacted_and_do_not_abort_retryable_errors() {
        let line = |session: &str, small: bool, error: &str| format!(
            "timestamp=2026-11-08T00:00:00Z level=ERROR run=test message=\"stream error\" session.id={session} small={small} error.error=\"{error}\""
        );
        let quota = line("target", false, "Free usage limit reached. apiKey=DO_NOT_DISPLAY");
        let visible = super::opencode_terminal_prompt_failure(&quota, "target").unwrap();
        assert!(visible.contains("免费额度"));
        assert!(!visible.contains("apiKey") && !visible.contains("DO_NOT_DISPLAY"));
        for ignored in [line("target-other", false, "Free usage limit reached"), line("other", false, "Free usage limit reached"), line("target", true, "Free usage limit reached"), line("target", false, "429 rate limit, retrying"), line("target", false, "temporary stream failure"), "tool output: Free usage limit reached".into()] {
            assert!(super::opencode_terminal_prompt_failure(&ignored, "target").is_none());
        }
        assert!(super::opencode_terminal_prompt_failure(&quota, "").is_none());
        for (error, expected) in [("invalid_api_key secret", "认证失败"), ("ProviderModelNotFoundError secret", "模型不存在"), ("insufficient_quota secret", "余额不足")] {
            let message = super::opencode_terminal_prompt_failure(&line("target", false, error), "target").unwrap();
            assert!(message.contains(expected));
            assert!(!message.contains("secret"));
        }
    }


    #[cfg(windows)]
    #[test]
    fn managed_protocol_error_with_rejected_cleanup_is_quarantined_and_typed() {
        let root=std::env::temp_dir().join(format!("acp-owned-stop-{}",uuid::Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();
        let script=root.join("reject-prompt.cjs");
        std::fs::write(&script,r#"const rl=require('readline').createInterface({input:process.stdin});const send=(id,result)=>process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');rl.on('line',line=>{const f=JSON.parse(line);if(f.id===undefined)return;if(f.method==='initialize')send(f.id,{protocolVersion:1,agentCapabilities:{loadSession:true}});else if(f.method==='session/new')send(f.id,{sessionId:'stop-fixture',modes:{currentModeId:'plan',availableModes:[{id:'plan',name:'Plan'}]}});else if(f.method==='session/prompt')process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:f.id,error:{code:-32001,message:'fixture prompt rejected'}})+'\n');else send(f.id,{})});setInterval(()=>{},1000)"#).unwrap();
        let mut command=Command::new("node");command.arg(&script);
        let process_id=Arc::new(AtomicU32::new(0));
        let mut connection=AcpConnection::spawn_for_with_cli_identity(ManagedAcpKind::ClaudeCode,root.to_str().unwrap(),None,Vec::new(),&["plan"],false,false,Arc::clone(&process_id),None,Some(command),None).unwrap();
        let pid=connection.child.id();
        let protocol_error=connection.prompt("test cleanup").unwrap_err();
        assert!(protocol_error.to_string().contains("fixture prompt rejected"));
        let denied=TestStopFailureGuard::new(pid);
        let error=finish_managed_connection(connection,Err(protocol_error)).err().expect("cleanup must fail");
        assert!(error.downcast_ref::<UnverifiedStop>().is_some(),"{error:#}");
        assert_eq!(process_id.load(Ordering::Acquire),pid);
        assert!(QUARANTINED_ACP.lock().unwrap().iter().any(|owner|owner.child.id()==pid));
        drop(denied);
        let mut owner={let mut owners=QUARANTINED_ACP.lock().unwrap();let index=owners.iter().position(|owner|owner.child.id()==pid).unwrap();owners.remove(index)};
        owner.stop_checked().unwrap();drop(owner);
        assert_eq!(process_id.load(Ordering::Acquire),0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn process_parent_validation_rejects_pid_reuse_and_accepts_same_lifetime_children() {
        assert!(validate_process_edge(10, 10, 100, Some(200), 150).is_ok());
        assert!(validate_process_edge(10, 10, 100, None, 250).is_ok());
        assert!(validate_process_edge(10, 99, 100, Some(200), 150).is_err());
        assert!(validate_process_edge(10, 10, 100, Some(200), 201).is_err());
        assert!(validate_process_edge(10, 10, 100, None, 99).is_err());
    }

    #[cfg(windows)]
    #[test]
    fn pid_only_stop_cannot_kill_an_unregistered_live_process() {
        let mut child = Command::new("node").args(["-e", "setInterval(()=>{},1000)"]).spawn().unwrap();
        let error = terminate_process_tree_checked(child.id()).unwrap_err();
        assert!(error.downcast_ref::<UnverifiedStop>().is_some());
        assert!(child.try_wait().unwrap().is_none());
        child.kill().unwrap(); child.wait().unwrap();
    }
    use super::*;

    #[test]
    fn codex_packaged_wrapper_materialization_and_npx_arguments_keep_bridge() {
        let root = std::env::temp_dir().join(format!("crewvela adapter {}", uuid::Uuid::new_v4()));
        let wrapper = materialize_codex_wrapper_at(&root).unwrap();
        assert_eq!(std::fs::read_to_string(&wrapper).unwrap(), include_str!("../../tools/codex-acp-wrapper.mjs"));
        std::fs::write(&wrapper, "old bridge").unwrap();
        assert_eq!(materialize_codex_wrapper_at(&root).unwrap(), wrapper);
        assert!(std::fs::read_to_string(&wrapper).unwrap().contains("resolveAdapterEntry"));
        let npm_dir = root.join("node_modules/npm/bin");
        std::fs::create_dir_all(&npm_dir).unwrap();
        std::fs::write(npm_dir.join("npx-cli.js"), "// fixture").unwrap();
        let npx = root.join(if cfg!(windows) { "npx.cmd" } else { "npx" });
        let node = root.join(if cfg!(windows) { "node.exe" } else { "node" });
        let command = codex_npx_command(&npx, &node, &wrapper).unwrap();
        let args: Vec<_> = command.get_args().map(|value| value.to_string_lossy().into_owned()).collect();
        assert!(args.windows(3).any(|values| values == ["--yes", "--package", CODEX_ACP_ADAPTER_PACKAGE]));
        assert_eq!(args.last().unwrap(), CODEX_ACP_NPX_BOOTSTRAP);
        assert_eq!(args[args.len()-2], "--call");
        assert!(!args.iter().any(|arg| arg.contains(&wrapper.display().to_string())));
        assert!(command.get_envs().any(|(key, value)| key == "CREWVELA_CODEX_ACP_WRAPPER" && value == Some(wrapper.as_os_str())));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn codex_npx_shell_bootstrap_preserves_spaces_and_metacharacters() {
        let root = std::env::temp_dir().join(format!("crewvela npx {}", uuid::Uuid::new_v4()));
        let runtime = root.join("Runtime with spaces");
        let npm_dir = runtime.join("node_modules/npm/bin");
        std::fs::create_dir_all(&npm_dir).unwrap();
        let node = runtime.join("node.exe");
        std::fs::copy(which::which("node").unwrap(), &node).unwrap();
        // Reproduce npm exec's join-then-shell boundary without network/package
        // installation. The separate opt-in smoke check covers the real npx.
        std::fs::write(npm_dir.join("npx-cli.js"), r#"
const {spawnSync}=require('node:child_process');
const args=process.argv.slice(2), at=args.indexOf('--call');
const text=at>=0 ? args[at+1] : args.slice(args.indexOf('--')+1).join(' ');
const result=spawnSync(text,{shell:true,env:process.env,encoding:'utf8'});
process.stdout.write(result.stdout||''); process.stderr.write(result.stderr||'');
process.exit(result.status??1);
"#).unwrap();
        let directory = root.join("wrapper & %PATH% 中文");
        std::fs::create_dir_all(&directory).unwrap();
        let wrapper = directory.join("bridge.mjs");
        std::fs::write(&wrapper, "process.stdout.write(JSON.stringify({path:process.env.CREWVELA_CODEX_ACP_WRAPPER}));").unwrap();
        let mut command = codex_npx_command(&runtime.join("npx.cmd"), &node, &wrapper).unwrap();
        let output = command.output().unwrap();
        assert!(output.status.success(), "npx bootstrap must not split the executable path or interpret wrapper metacharacters");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["path"], wrapper.display().to_string());
        assert_eq!(root.parent().unwrap(), std::env::temp_dir());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires installed Node/npm and cached/network Codex ACP; isolated CODEX_HOME, no credentials or model prompt"]
    fn installed_codex_npx_initializes_without_credentials_or_model_call() {
        let root = std::env::temp_dir().join(format!("crewvela real adapter {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("codex")).unwrap();
        let node = which::which("node").unwrap();
        let npx = which::which(if cfg!(windows) { "npx.cmd" } else { "npx" }).unwrap();
        let wrapper = materialize_codex_wrapper_at(&root.join("wrapper & spaces 中文")).unwrap();
        let mut command = codex_npx_command(&npx, &node, &wrapper).unwrap();
        command.current_dir(&root).env("CREWVELA_CODEX_ACP_BRIDGE", "1")
            .env("CODEX_HOME", root.join("codex"))
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        if let Some(codex) = crate::multi_agent::resolve_runtime_executable(crate::multi_agent::AgentRuntimeKind::Codex) {
            command.env("CODEX_PATH", codex);
        }
        let (mut child, identity) = spawn_owned_command(&mut command).unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let _stderr = capture_stderr(child.stderr.take().unwrap());
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(value) = serde_json::from_str::<Value>(&line) { let _ = sender.send(value); }
            }
        });
        let result = (|| -> Result<()> {
            serde_json::to_writer(&mut stdin, &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"crewvela-isolated-smoke","version":"1"}}}))?;
            stdin.write_all(b"\n")?; stdin.flush()?;
            let deadline = Instant::now() + Duration::from_secs(40);
            loop {
                let reply = receiver.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .context("真实 npx ACP 初始化前退出或超时")?;
                if reply.get("id") == Some(&json!(1)) {
                    if reply.get("result").is_none() { return Err(anyhow!("真实 Codex ACP 返回初始化错误，未发送任何模型请求")); }
                    return Ok(());
                }
            }
        })();
        drop(stdin);
        stop_child_checked(&mut child).unwrap();
        drop(identity);
        assert_eq!(root.parent().unwrap(), std::env::temp_dir());
        // Job termination is verified above; Windows may release SQLite mapped
        // files just after the process exit notification. Retry only sharing
        // violations in this isolated test directory, never skip cleanup.
        let cleanup_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match std::fs::remove_dir_all(&root) {
                Ok(()) => break,
                Err(error) if error.raw_os_error() == Some(32) && Instant::now() < cleanup_deadline => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => panic!("isolated ACP test cleanup failed: {error}"),
            }
        }
        result.unwrap();
    }

    #[test]
    fn stderr_capture_truncates_large_unicode_diagnostics_without_panicking() {
        let input = format!("{}\n{}\n", "中文日志🛠️".repeat(3000), "后续错误".repeat(4000));
        let captured = capture_stderr(std::io::Cursor::new(input.into_bytes()));
        let deadline = Instant::now() + Duration::from_secs(2);
        while Arc::strong_count(&captured) > 1 {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        let value = captured.lock().expect("Unicode truncation must not poison the capture lock");
        assert!(value.len() <= 12_000);
        assert!(value.contains("后续错误"));
    }


    #[test]
    #[ignore = "requires installed Codex/adapter and configured local ai-memory; stop ai-memory before running"]
    fn codex_chat_open_recovers_configured_stopped_ai_memory() {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let before = runtime.block_on(crate::ai_memory::ai_memory_status()).unwrap();
        assert!(before.installed, "install ai-memory explicitly before this check");
        assert!(!before.server_running, "stop ai-memory before testing startup recovery");
        let cwd = std::env::current_dir().unwrap();
        let connection = AcpConnection::spawn_native_codex_chat(
            cwd.to_str().unwrap(), None, Vec::new(), Box::new(|_, _| Ok(false)),
            Arc::new(AtomicU32::new(0)), None,
        ).expect("real Codex ACP session/new handshake must succeed without sending a prompt");
        let after = runtime.block_on(crate::ai_memory::ai_memory_status()).unwrap();
        connection.terminate();
        assert!(after.server_running, "opening Codex must recover the configured local service");
        assert!(after.managed_by_crewvela, "recovered service must retain explicit stop support");
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn codex_acp_npx_path_with_spaces_completes_new_and_resume_handshake() {
        let root = std::env::temp_dir().join(format!("CrewVela ACP (&) {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/codex-acp-agent.mjs"),
            root.join("peer.mjs"),
        ).unwrap();
        let script = root.join("npx.cmd");
        // The .cmd must not run. Exercise the actual npm-entry fallback layout.
        std::fs::write(&script, "@exit /b 41\r\n").unwrap();
        let npm_bin = root.join("node_modules/npm/bin");
        std::fs::create_dir_all(&npm_bin).unwrap();
        std::fs::write(npm_bin.join("npx-cli.js"), format!(
            "const assert = require('node:assert/strict'); assert.deepEqual(process.argv.slice(2), ['--yes', {}]); import(require('node:url').pathToFileURL(require('node:path').join(__dirname, '../../../peer.mjs')).href);",
            serde_json::to_string(CODEX_ACP_ADAPTER_PACKAGE).unwrap(),
        )).unwrap();
        for resume in [None, Some("codex-fake")] {
            let mut command = npx_command(&script).unwrap();
            command.arg("--yes").arg(CODEX_ACP_ADAPTER_PACKAGE);
            let connection = AcpConnection::spawn_native_codex_chat(
                root.to_str().unwrap(), resume, Vec::new(), Box::new(|_, _| Ok(false)),
                Arc::new(AtomicU32::new(0)), Some(command),
            ).unwrap();
            assert_eq!(connection.session_id(), "codex-fake");
            assert_eq!(connection.last_start_resumed, resume.is_some());
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn npx_cmd_without_npm_entry_reports_installation_error() {
        let root = std::env::temp_dir().join(format!("CrewVela missing npm {}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let error = npx_command(&root.join("npx.cmd")).unwrap_err().to_string();
        assert!(error.contains("未找到 npx 的 npm 入口"));
        assert!(error.contains("Node.js/npm"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn harness_paths_convert_drive_and_unc_without_changing_devices() {
        assert_eq!(harness_windows_path(r"\\?\G:\tmp"), r"G:\tmp");
        assert_eq!(harness_windows_path(r"\\?\G:\"), r"G:\");
        assert_eq!(harness_windows_path(r"\\?\UNC\server\share\tmp"), r"\\server\share\tmp");
        assert_eq!(harness_windows_path(r"\\?\UNC\server\share\"), r"\\server\share\");
        assert_eq!(harness_windows_path(r"G:\tmp"), r"G:\tmp");
        assert_eq!(harness_windows_path(r"\\?\Volume{123}\tmp"), r"\\?\Volume{123}\tmp");
    }

    #[test]
    fn harness_canonical_directory_keeps_directory_validation() {
        let root = std::env::temp_dir().join(format!("harness-path-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let canonical = canonical_harness_directory(root.to_str().unwrap()).unwrap();
        assert_eq!(Path::new(&canonical).canonicalize().unwrap(), root.canonicalize().unwrap());
        #[cfg(target_os = "windows")]
        assert!(!canonical.starts_with(r"\\?\"));
        std::fs::write(root.join("file"), "file").unwrap();
        assert!(canonical_harness_directory(root.join("file").to_str().unwrap()).is_err());
        assert!(canonical_harness_directory(root.join("missing").to_str().unwrap()).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn deepseek_acp_normalizes_verbatim_cwd_for_new_and_resume() {
        let verbatim = Path::new(env!("CARGO_MANIFEST_DIR")).canonicalize().unwrap();
        let cwd = verbatim.to_str().unwrap();
        assert!(cwd.starts_with(r"\\?\"));
        for resume in [None, Some("native-fake")] {
            let mut command = Command::new(which::which("node").unwrap());
            command.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/native-harness-acp.mjs"));
            let connection = AcpConnection::spawn_native_deepseek_harness(
                cwd, resume, Vec::new(), Box::new(|_, _| Ok(false)),
                Arc::new(AtomicU32::new(0)), Some(command),
            ).unwrap();
            assert_eq!(connection.cwd(), canonical_harness_directory(cwd).unwrap());
            assert_eq!(connection.session_id(), "native-fake");
            assert_eq!(connection.last_start_resumed, resume.is_some());
        }
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn native_chat_stdio_mcp_servers_pass_without_http_capability() {
        // native-harness fixture 不声明 mcpCapabilities.http。旧门禁对任何非空
        // mcp_servers 一刀切拒启；修正后 stdio-only 条目（浏览器控制形态）应放行。
        let cwd = env!("CARGO_MANIFEST_DIR");
        let mut command = Command::new(which::which("node").unwrap());
        command.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/native-harness-acp.mjs"));
        let connection = AcpConnection::spawn_native_deepseek_harness(
            cwd, None,
            vec![json!({ "name": "browser-control", "command": "node", "args": ["cli.js"] })],
            Box::new(|_, _| Ok(false)),
            Arc::new(AtomicU32::new(0)), Some(command),
        ).unwrap();
        assert_eq!(connection.session_id(), "native-fake");
    }

    fn fake_deepseek(resume: Option<&str>, mode: &str, approve: bool) -> Result<AcpConnection> {
        let mut command = Command::new(which::which("node")?);
        command
            .arg(
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .unwrap()
                    .join("tests/fixtures/deepseek-acp-agent.mjs"),
            )
            .arg(mode);
        AcpConnection::spawn_for_with_cli_identity(
            ManagedAcpKind::DeepSeekHarness,
            env!("CARGO_MANIFEST_DIR"),
            resume,
            vec![managed_mcp_server("http://127.0.0.1/mcp", "test")],
            &[mode],
            approve,
            true,
            Arc::new(AtomicU32::new(0)),
            None,
            Some(command),
            None,
        )
    }

    #[test]
    fn deepseek_keyless_acp_roundtrip_and_resume() {
        let mut connection = fake_deepseek(None, "workspace-write", true).unwrap();
        let model = resolve_deepseek_model(&connection.config_options, "model-a").unwrap();
        assert_eq!(model, r#"["test","model-a"]"#);
        connection
            .request(
                "session/set_config_option",
                json!({"sessionId": "fake", "configId": "model", "value": model}),
                START_TIMEOUT,
                None,
            )
            .unwrap();
        assert_eq!(connection.prompt("test").unwrap().text, "approved");
        connection
            .request(
                "session/close",
                json!({"sessionId": "fake"}),
                START_TIMEOUT,
                None,
            )
            .unwrap();
        drop(connection);
        let mut resumed = fake_deepseek(Some("fake"), "workspace-write", false).unwrap();
        assert!(resumed.last_start_resumed);
        assert_eq!(resumed.session_id, "fake");
        resumed.permission_handler = Some(Box::new(|request, _| {
            assert_eq!(
                request
                    .pointer("/params/toolCall/title")
                    .and_then(Value::as_str),
                Some("bash")
            );
            assert_eq!(
                request
                    .pointer("/params/toolCall/rawInput/command")
                    .and_then(Value::as_str),
                Some("touch example.txt")
            );
            Ok(true)
        }));
        assert_eq!(resumed.prompt("test").unwrap().text, "approved");
        assert!(fake_deepseek(Some("missing"), "workspace-write", false).is_err());
    }

    #[test]
    fn deepseek_readonly_rejects_and_prompt_cancels() {
        let mut connection = fake_deepseek(None, "read-only", false).unwrap();
        assert_eq!(connection.prompt("test").unwrap().text, "rejected");
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&cancelled);
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(150));
            flag.store(true, Ordering::Release);
        });
        assert!(connection
            .prompt_cancellable("cancel", Some(&cancelled))
            .unwrap_err()
            .to_string()
            .contains("取消"));
        thread.join().unwrap();
    }

    #[test]
    fn deepseek_model_selection_rejects_unknown_and_ambiguous() {
        let options = vec![json!({"id": "model", "options": [{"options": [
            {"name": "shared", "value": "[\"a\",\"shared\"]"},
            {"name": "shared", "value": "[\"b\",\"shared\"]"}
        ]}]})];
        assert!(resolve_deepseek_model(&options, "unknown").is_err());
        assert!(resolve_deepseek_model(&options, "shared").is_err());
        assert_eq!(
            resolve_deepseek_model(&options, "a/shared").unwrap(),
            "[\"a\",\"shared\"]"
        );
    }

    #[test]
    #[ignore = "需要安装 dsh；只握手和选择当前模型，不提交 prompt，不调用模型"]
    fn deepseek_installed_control_surface_without_model_call() {
        let cwd = std::env::temp_dir();
        let cwd = cwd.to_str().unwrap();
        let create = |resume: Option<&str>| {
            AcpConnection::spawn_for(
                ManagedAcpKind::DeepSeekHarness,
                cwd,
                resume,
                vec![],
                &["read-only"],
                false,
                true,
                Arc::new(AtomicU32::new(0)),
            )
        };
        let mut connection = create(None).unwrap();
        let session_id = connection.session_id.clone();
        let current = connection
            .config_options
            .iter()
            .find(|option| option.get("id").and_then(Value::as_str) == Some("model"))
            .unwrap()
            .get("currentValue")
            .unwrap()
            .clone();
        connection
            .request(
                "session/set_config_option",
                json!({"sessionId": session_id, "configId": "model", "value": current}),
                START_TIMEOUT,
                None,
            )
            .unwrap();
        connection
            .request(
                "session/close",
                json!({"sessionId": session_id}),
                START_TIMEOUT,
                None,
            )
            .unwrap();
        drop(connection);
        let mut connection = create(Some(&session_id)).unwrap();
        assert!(connection.last_start_resumed);
        assert_eq!(connection.session_id, session_id);
        connection
            .request(
                "session/close",
                json!({"sessionId": session_id}),
                START_TIMEOUT,
                None,
            )
            .unwrap();
    }

    #[test]
    fn acp_cli_sender_identity_is_inherited_without_cross_session_leaks() {
        let parent_identity = std::env::var_os("CREWVELA_SESSION_ID");
        let executable = std::env::current_exe().unwrap();
        let mut children = Vec::new();
        for sender in ["claude-acp-sender", "codex-acp-sender"] {
            #[cfg(target_os = "windows")]
            let mut command = {
                use std::os::windows::process::CommandExt;
                let mut command = Command::new("powershell.exe");
                command.args(["-NoProfile", "-Command",
                    "[Console]::WriteLine($env:CREWVELA_SESSION_ID); [Console]::WriteLine($env:CREWVELA_CLI)"]);
                command.creation_flags(0x08000000);
                command
            };
            #[cfg(not(target_os = "windows"))]
            let mut command = {
                let mut command = Command::new("sh");
                command.args([
                    "-c",
                    "printf '%s\\n%s\\n' \"$CREWVELA_SESSION_ID\" \"$CREWVELA_CLI\"",
                ]);
                command
            };
            configure_session_cli_identity(&mut command, sender).unwrap();
            children.push((
                sender,
                command
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap(),
            ));
        }
        for (sender, child) in children {
            let output = child.wait_with_output().unwrap();
            assert!(output.status.success());
            let text = String::from_utf8(output.stdout).unwrap();
            let mut lines = text.lines();
            assert_eq!(lines.next(), Some(sender));
            assert_eq!(lines.next(), Some(executable.to_str().unwrap()));
        }
        assert_eq!(std::env::var_os("CREWVELA_SESSION_ID"), parent_identity);
    }

    #[test]
    fn builds_scoped_http_mcp_server() {
        assert_eq!(
            managed_mcp_server("http://127.0.0.1:54318/mcp", "secret"),
            json!({
                "type": "http",
                "name": "crewvela",
                "url": "http://127.0.0.1:54318/mcp",
                "headers": [{
                    "name": "Authorization",
                    "value": "Bearer secret"
                }]
            })
        );
    }

    #[test]
    fn resumed_full_access_session_cannot_continue_after_failed_downgrade() {
        let directory = std::env::temp_dir().join(format!("acp-mode-downgrade-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let script = directory.join("peer.mjs");
        std::fs::write(&script, r#"import readline from 'node:readline';
const send = (id,result) => process.stdout.write(JSON.stringify({jsonrpc:'2.0',id,result})+'\n');
for await (const line of readline.createInterface({input:process.stdin})) {
 const frame=JSON.parse(line);
 if(frame.method==='initialize') send(frame.id,{protocolVersion:1,agentCapabilities:{loadSession:true},agentInfo:{name:'mode-fixture'}});
 else if(frame.method==='session/load'||frame.method==='session/new') send(frame.id,{sessionId:'fixture-mode',modes:{currentModeId:'bypassPermissions',availableModes:[{id:'plan',name:'Plan'},{id:'bypassPermissions',name:'Full'}]}});
 else if(frame.method==='session/set_mode') process.stdout.write(JSON.stringify({jsonrpc:'2.0',id:frame.id,error:{code:-32603,message:'fixture rejects downgrade'}})+'\n');
 else if(frame.id!==undefined) send(frame.id,{});
}
"#).unwrap();
        let mut command = Command::new(which::which("node").unwrap());
        command.arg(&script);
        let result = AcpConnection::spawn_for_with_cli_identity(ManagedAcpKind::ClaudeCode,
            directory.to_str().unwrap(), Some("fixture-mode"), Vec::new(), &["plan"], false, false,
            Arc::new(AtomicU32::new(0)), None, Some(command), None);
        assert!(result.is_err(), "known unrestricted session must not masquerade as restricted");
        assert!(format!("{:#}", result.err().unwrap()).contains("无法确认已降权"));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn maps_managed_permissions_to_adapter_modes() {
        assert_eq!(
            ManagedAcpMode::ReadOnly.candidates(),
            &["read-only", "plan"]
        );
        assert!(ManagedAcpMode::Auto
            .candidates()
            .contains(&"agent-full-access"));
        assert_eq!(
            ManagedAcpMode::FullAccess.candidates(),
            &["agent-full-access", "bypassPermissions"]
        );
    }
    #[test]
    fn parses_available_commands() {
        let commands = parse_commands(&json!({
            "sessionUpdate": "available_commands_update",
            "availableCommands": [{
                "name": "model",
                "description": "Switch model",
                "input": { "hint": "model name" }
            }, {
                "name": "model",
                "description": "duplicate skill shadowing the builtin",
                "input": { "hint": "ignored" }
            }]
        }));
        assert_eq!(
            commands,
            vec![AcpCommand {
                name: "model".to_string(),
                description: "Switch model".to_string(),
                input_hint: Some("model name".to_string()),
            }]
        );
    }

    #[test]
    fn prefers_persistent_permission() {
        let request = json!({
            "params": {
                "options": [
                    { "optionId": "once", "kind": "allow_once" },
                    { "optionId": "always", "kind": "allow_always" }
                ]
            }
        });
        assert_eq!(
            choose_permission_option(&request, true).as_deref(),
            Some("always")
        );
    }

    #[test]
    fn rejects_permissions_when_auto_approval_is_disabled() {
        let request = json!({
            "params": {
                "options": [
                    { "optionId": "once", "kind": "allow_once" },
                    { "optionId": "reject", "kind": "reject_once" }
                ]
            }
        });

        assert_eq!(
            choose_permission_option(&request, false).as_deref(),
            Some("reject"),
        );
    }

    #[test]
    fn codex_acp_tool_environment_preserves_config_and_isolates_senders() {
        for identity in ["sender-a", "sender-b"] {
            let mut command = Command::new("codex-acp");
            command.env("CODEX_CONFIG", r#"{"model":"existing-model","shell_environment_policy.inherit":"core","shell_environment_policy.set.KEEP":"yes"}"#);
            configure_codex_acp_cli_identity(&mut command, identity).unwrap();
            let raw = command
                .get_envs()
                .find(|(key, _)| *key == "CODEX_CONFIG")
                .unwrap()
                .1
                .unwrap();
            let config: Value = serde_json::from_str(&raw.to_string_lossy()).unwrap();
            assert_eq!(config["model"], "existing-model");
            assert_eq!(config["shell_environment_policy.inherit"], "core");
            assert_eq!(config["shell_environment_policy.set.KEEP"], "yes");
            assert_eq!(
                config["shell_environment_policy.set.CREWVELA_SESSION_ID"],
                identity
            );
            assert_eq!(
                config["shell_environment_policy.set.CREWVELA_CLI"],
                std::env::current_exe().unwrap().display().to_string()
            );
        }
        let mut invalid = Command::new("codex-acp");
        invalid.env("CODEX_CONFIG", "[]");
        assert!(configure_codex_acp_cli_identity(&mut invalid, "sender").is_err());
    }

    #[test]
    fn deny_never_falls_back_to_allow_options() {
        let request = json!({
            "params": {
                "options": [
                    { "optionId": "once", "kind": "allow_once" },
                    { "optionId": "always", "kind": "allow_always" }
                ]
            }
        });

        assert_eq!(choose_permission_option(&request, false), None);
        assert_eq!(
            choose_permission_option(&request, true).as_deref(),
            Some("always")
        );
    }

    #[test]
    fn read_only_crewvela_mcp_calls_can_be_approved_without_full_permissions() {
        let update = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "read-task",
            "kind": "execute",
            "rawInput": { "server": "crewvela", "tool": "cc_task_read", "arguments": {} }
        });
        let write_update = json!({
            "sessionUpdate": "tool_call",
            "toolCallId": "write-memory",
            "kind": "execute",
            "rawInput": { "server": "crewvela", "tool": "cc_memory_put", "arguments": {} }
        });
        let request = json!({ "params": { "toolCall": { "toolCallId": "read-task" } } });
        let write_request = json!({ "params": { "toolCall": { "toolCallId": "write-memory" } } });
        let mut allowed = HashSet::new();
        if is_read_only_crewvela_mcp_call(&update) {
            allowed.insert("read-task".to_string());
        }
        if is_read_only_crewvela_mcp_call(&write_update) {
            allowed.insert("write-memory".to_string());
        }

        assert!(should_allow_permission(&request, false, &allowed));
        assert!(!should_allow_permission(&write_request, false, &allowed));
    }

    #[test]
    fn collects_agent_message_chunks() {
        let mut collector = PromptCollector::default();
        collector.push_text("hello ");
        collector.push_text("world");
        assert_eq!(collector.text, "hello world");
    }

    #[test]
    fn builds_session_cancel_notification() {
        assert_eq!(
            cancel_notification("session-42"),
            json!({
                "jsonrpc": "2.0",
                "method": "session/cancel",
                "params": { "sessionId": "session-42" }
            })
        );
    }

    #[test]
    #[ignore = "requires CREWVELA_RUN_REAL_COLLABORATION=1, installed Codex/Claude adapters, login and network; at most three model prompts"]
    fn real_models_exchange_scoped_mcp_messages_between_two_agents() {
        use crate::multi_agent::{
            AgentRole, AgentRuntimeKind, ClaimTaskInput, CollaborationBridge, CreateAgentInput,
            CreateProjectInput, CreateTaskInput, MultiAgentStore, PermissionProfile, ProjectStatus, WorkspaceMode,
        };
        assert_eq!(
            std::env::var("CREWVELA_RUN_REAL_COLLABORATION").as_deref(),
            Ok("1"),
            "explicitly opt into three real model prompts"
        );
        let root = std::env::temp_dir().join(format!("crewvela-real-team-{}", uuid::Uuid::new_v4()));
        let alpha = root.join("alpha");
        let beta = root.join("beta");
        std::fs::create_dir_all(&alpha).unwrap();
        std::fs::create_dir_all(&beta).unwrap();
        let store = MultiAgentStore::open_at(root.join("control.db")).unwrap();
        let project = store.create_project(CreateProjectInput {
            name: "isolated MCP model acceptance".into(),
            goal: "exchange two harmless random test messages".into(),
            root_path: root.display().to_string(),
            default_branch: "main".into(),
        }).unwrap();
        store.set_project_status(&project.id, ProjectStatus::Active).unwrap();
        let bridge = CollaborationBridge::start(store.clone()).unwrap();
        let participant = |name: &str, runtime_kind| {
            let agent = store.create_agent(CreateAgentInput {
                project_id: project.id.clone(), name: name.into(), runtime_kind,
                role: AgentRole::Worker, description: "isolated acceptance".into(),
                persona: Default::default(), persona_custom: String::new(), avatar_id: None,
                model: None, permission_profile: PermissionProfile::Default, max_concurrency: 1,
            }).unwrap();
            let task = store.create_task(CreateTaskInput {
                project_id: project.id.clone(), parent_id: None, stage: 0,
                title: format!("{name} isolated mailbox"), description: String::new(),
                acceptance_criteria: "one scoped message".into(), verification_command: None,
                assignee_agent_id: Some(agent.id.clone()), reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::ReadOnly, priority: 0,
                required_capabilities: Vec::new(), max_attempts: 1, retry_backoff_seconds: 0,
            }).unwrap();
            let run = store.claim_task(ClaimTaskInput {
                task_id: task.id.clone(), agent_id: agent.id.clone(), lease_seconds: 300,
            }).unwrap();
            (agent, task, run)
        };
        let (a, task_a, run_a) = participant("alpha-codex", AgentRuntimeKind::Codex);
        let (b, task_b, run_b) = participant("beta-claude", AgentRuntimeKind::ClaudeCode);
        let lease_a = bridge.register_run(&project.id, &task_a.id, &run_a.id, &a.id, &alpha);
        let lease_b = bridge.register_run(&project.id, &task_b.id, &run_b.id, &b.id, &beta);
        let access_a = lease_a.access();
        let access_b = lease_b.access();
        assert_ne!(access_a.token, access_b.token);

        fn message_permissions() -> ManagedPermissionHandler {
            Box::new(|request, _| {
                let tool = &request["params"]["toolCall"];
                let raw = &tool["rawInput"];
                let structured = raw["server"] == "crewvela"
                    && matches!(raw["tool"].as_str(), Some("cc_message_send" | "cc_thread_read"));
                let named = [tool["title"].as_str(), raw["name"].as_str()]
                    .into_iter().flatten().any(|name| matches!(name,
                        "mcp__crewvela__cc_message_send" | "mcp__crewvela__cc_thread_read"));
                if !(structured || named) {
                    eprintln!("Denied non-acceptance permission: kind={:?}, title={:?}",
                        tool["kind"].as_str(), tool["title"].as_str());
                }
                Ok(structured || named)
            })
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        let pid_a = Arc::new(AtomicU32::new(0));
        let pid_b = Arc::new(AtomicU32::new(0));
        struct Deadline(std::sync::mpsc::Sender<()>);
        impl Drop for Deadline {
            fn drop(&mut self) { let _ = self.0.send(()); }
        }
        let (done, finished) = std::sync::mpsc::channel();
        let _deadline = Deadline(done);
        let cancellation = cancelled.clone();
        let processes = [pid_a.clone(), pid_b.clone()];
        std::thread::spawn(move || {
            if matches!(finished.recv_timeout(Duration::from_secs(240)), Err(std::sync::mpsc::RecvTimeoutError::Timeout)) {
                cancellation.store(true, Ordering::Relaxed);
                for process in processes {
                    let _ = terminate_acp_process_checked(&process);
                }
            }
        });
        let mut sender = AcpConnection::spawn_native_codex_chat(
            &alpha.display().to_string(), None,
            vec![managed_mcp_server(&format!("{}/mcp", access_a.endpoint), &access_a.token)],
            message_permissions(), pid_a, None,
        ).expect("real Codex ACP startup failed");
        let mut receiver = AcpConnection::spawn_native_claude_chat(
            &beta.display().to_string(), None,
            vec![managed_mcp_server(&format!("{}/mcp", access_b.endpoint), &access_b.token)],
            message_permissions(), pid_b, None,
        ).expect("real Claude ACP startup failed");
        let sender_tools = Arc::new(Mutex::new(Vec::<Value>::new()));
        let observed_tools = sender_tools.clone();
        sender.set_update_handler(Box::new(move |update| {
            if matches!(update["sessionUpdate"].as_str(), Some("tool_call" | "tool_call_update")) {
                observed_tools.lock().unwrap().push(json!({
                    "toolCallId": update["toolCallId"], "title": update["title"],
                    "status": update["status"],
                    "isThreadRead": update.to_string().contains("cc_thread_read"),
                }));
            }
        }));
        let marker = format!("CREWVELA_PROBE_{}", uuid::Uuid::new_v4().simple());
        let first = sender.prompt_cancellable(&format!(
            "This is an isolated communication acceptance test. Use only the crewvela MCP tool cc_message_send. \
             Send exactly {marker} as body to targetAgentId {}. Do not use shell, files, memory or any other tools. \
             After the tool succeeds reply SENT.", b.id
        ), Some(&cancelled)).expect("real sender model call failed");
        assert!(!first.cancelled);
        let snapshot = store.snapshot(&project.id).unwrap();
        let outbound = snapshot.messages.iter().find(|message| message.body == marker)
            .expect("sender model did not persist its MCP message");
        assert_eq!(outbound.sender_id.as_deref(), Some(a.id.as_str()));
        assert_eq!(outbound.target_agent_id.as_deref(), Some(b.id.as_str()));
        assert_eq!(outbound.task_id.as_deref(), Some(task_a.id.as_str()));
        let second = receiver.prompt_cancellable(&format!(
            "This is an isolated communication acceptance test. Use only crewvela MCP tools cc_thread_read and cc_message_send. \
             Read your thread, find the newest received body starting CREWVELA_PROBE_, then send body ACK: followed by that exact \
             received body to targetAgentId {}. Do not invent the received value. Do not use shell, files, memory or other tools. \
             Reply with only the received body after sending.", a.id
        ), Some(&cancelled)).expect("real receiving model call failed");
        assert!(!second.cancelled);
        assert!(second.text.contains(&marker), "receiver did not demonstrate reading its own thread");
        let snapshot = store.snapshot(&project.id).unwrap();
        let reply = snapshot.messages.iter().find(|message| {
            message.body.strip_prefix("ACK:").map(str::trim) == Some(marker.as_str())
        })
            .expect("receiving model did not persist its MCP acknowledgement");
        assert_eq!(reply.sender_id.as_deref(), Some(b.id.as_str()));
        assert_eq!(reply.target_agent_id.as_deref(), Some(a.id.as_str()));
        assert_eq!(reply.task_id.as_deref(), Some(task_b.id.as_str()));
        let message_count_before_read = snapshot.messages.len();
        let tools_before_read = sender_tools.lock().unwrap().len();
        let third = sender.prompt_cancellable(
            "Use only the crewvela MCP tool cc_thread_read. Read your thread, find the newest received body starting ACK:CREWVELA_PROBE_ (there may be a space after ACK:), and reply with that exact received body. Do not invent the body or send any new messages. Do not use shell, files, memory or other tools.",
            Some(&cancelled),
        ).expect("real sender could not read the reply");
        assert!(!third.cancelled);
        assert!(third.text.contains(&marker) && third.text.contains("ACK:"), "original sender did not read the acknowledgement from its own thread");
        let tool_events = sender_tools.lock().unwrap();
        let read_events = &tool_events[tools_before_read..];
        assert!(read_events.iter().any(|call| call["isThreadRead"] == true && read_events.iter().any(|event|
            event["toolCallId"] == call["toolCallId"] && event["status"] == "completed"
        )), "original sender must complete a real cc_thread_read tool call: {read_events:?}");
        let after_read = store.snapshot(&project.id).unwrap();
        assert_eq!(after_read.messages.len(), message_count_before_read, "reading the acknowledgement must not create a reply loop");
        assert_eq!(after_read.messages.iter().filter(|message| message.sender_type == "agent").count(), 2, "exactly one outbound message and one acknowledgement are allowed");
        eprintln!("REAL_TEAM_MCP_EVIDENCE: marker={marker}, outbound={}, reply={}, codex={}, claude={}, store={}",
            outbound.id, reply.id, a.id, b.id, root.join("control.db").display());
        std::fs::write(root.join("acceptance.json"), serde_json::to_vec_pretty(&json!({
            "passed": true, "marker": marker, "outboundId": outbound.id, "replyId": reply.id,
            "codexAgentId": a.id, "claudeAgentId": b.id, "senderReadTools": read_events,
            "prompts": 3, "agentMessages": 2,
        })).unwrap()).unwrap();
        drop(sender);
        drop(receiver);
        drop(lease_a);
        drop(lease_b);
        eprintln!("REAL_TEAM_MCP_OK: three model prompts, Codex send -> Claude read/reply -> Codex read, independent sessions and verified identities");
        // The bridge listener owns its SQLite handle until process exit. Leave
        // this uniquely named OS temp fixture for ordinary temp-directory cleanup.
    }

    #[test]
    #[ignore = "requires the installed Claude ACP adapter, login, and network"]
    fn executes_real_claude_prompt_without_project_context() {
        let cwd = std::env::temp_dir().join(format!(
            "crewvela-claude-acp-prompt-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&cwd).unwrap();
        let mut connection = AcpConnection::spawn_for(
            ManagedAcpKind::ClaudeCode,
            &cwd.display().to_string(),
            None,
            Vec::new(),
            &["plan"],
            false,
            true,
            Arc::new(AtomicU32::new(0)),
        )
        .expect("Claude ACP adapter should start");
        let result = connection
            .prompt("Reply with exactly CREWVELA_CLAUDE_ACP_OK and do not use tools.")
            .expect("Claude ACP prompt should complete");
        let _ = std::fs::remove_dir_all(cwd);

        assert_eq!(result.text.trim(), "CREWVELA_CLAUDE_ACP_OK");
    }

    #[test]
    #[ignore = "requires the installed Claude ACP adapter and login"]
    fn starts_real_claude_adapter_without_prompt() {
        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("project root")
            .display()
            .to_string();
        let connection = AcpConnection::spawn_for(
            ManagedAcpKind::ClaudeCode,
            &cwd,
            None,
            vec![managed_mcp_server("http://127.0.0.1:9/mcp", "test")],
            &["bypassPermissions"],
            true,
            true,
            Arc::new(AtomicU32::new(0)),
        )
        .expect("Claude ACP adapter should start");

        assert!(!connection.session_id.is_empty());
        assert!(!connection.agent_name.is_empty());
    }

    #[test]
    #[ignore = "requires the installed Codex ACP adapter, login, and network"]
    fn executes_real_codex_prompt_without_project_context() {
        let cwd = std::env::temp_dir().join(format!("crewvela-acp-prompt-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cwd).unwrap();
        let mut connection = AcpConnection::spawn_for(
            ManagedAcpKind::Codex,
            &cwd.display().to_string(),
            None,
            Vec::new(),
            &["read-only"],
            false,
            true,
            Arc::new(AtomicU32::new(0)),
        )
        .expect("Codex ACP adapter should start");
        let result = connection
            .prompt("Reply with exactly CREWVELA_ACP_OK and do not use tools.")
            .expect("Codex ACP prompt should complete");
        let _ = std::fs::remove_dir_all(cwd);

        assert_eq!(result.text.trim(), "CREWVELA_ACP_OK");
    }

    #[test]
    #[ignore = "requires the installed Codex ACP adapter and login"]
    fn starts_real_codex_adapter() {
        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("project root")
            .display()
            .to_string();
        let connection = AcpConnection::spawn_for(
            ManagedAcpKind::Codex,
            &cwd,
            None,
            vec![managed_mcp_server("http://127.0.0.1:9/mcp", "test")],
            &["agent"],
            false,
            true,
            Arc::new(AtomicU32::new(0)),
        )
        .expect("Codex ACP adapter should start");

        assert!(!connection.session_id.is_empty());
        assert!(!connection.agent_name.is_empty());
    }

    #[test]
    #[ignore = "requires the installed Node.js ACP adapter"]
    fn starts_real_adapter_and_discovers_commands() {
        let cwd = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("project root")
            .display()
            .to_string();
        let mut connection = AcpConnection::spawn(&cwd, None, Arc::new(AtomicU32::new(0)))
            .expect("ACP adapter should start");
        assert!(!connection.session_id.is_empty());
        assert!(connection
            .commands
            .iter()
            .any(|command| command.name == "clear"));
        assert!(connection
            .commands
            .iter()
            .any(|command| command.name == "context"));
        assert!(connection.commands.len() > 1);
        let result = connection
            .prompt("/context")
            .expect("ACP should execute a built-in slash command");
        assert!(!result.text.is_empty());
    }
}
