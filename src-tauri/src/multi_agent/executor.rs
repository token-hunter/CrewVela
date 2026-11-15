use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, Read, Write};
use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use crate::acp::{execute_managed_acp, ManagedAcpKind, ManagedAcpMode, ManagedAcpOutcome, UnverifiedStop};
use crate::pty::PtyManager;
use crate::sessions::{NewAgentSession, SessionManager, SessionRecord, SessionStatus};

use super::{
    contract_requires_session_cli, AgentPersona, AgentProfile, AgentRole, AgentRuntimeKind,
    ArtifactRecord, ClaimTaskInput, CollaborationAccess, CollaborationBridge,
    CollaborationGovernance, CollaborationMessage, HeartbeatRunInput, MemoryEntry,
    MultiAgentProject, MultiAgentStore, MultiAgentTask, PermissionProfile,
    ProjectExecutionReceipt, ProjectStatus, RunStatus, SearchMemoryInput, UpdateRunInput,
    WatchdogDisposition, WorkspaceMode,
};

const RUN_TIMEOUT: Duration = Duration::from_secs(60 * 60);
const STALE_PROMPT_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const VISIBLE_RUNTIME_INTERRUPT_TIMEOUT: Duration = Duration::from_secs(3);
const SUPERVISOR_STOP_TIMEOUT: Duration = Duration::from_secs(5);
const STREAM_CAPTURE_PREFIX_BYTES: usize = 256 * 1024;
const STREAM_CAPTURE_TAIL_BYTES: usize = 4 * 1024 * 1024;
const STREAM_DRAIN_TIMEOUT: Duration = Duration::from_secs(2);
const STREAM_CANCEL_TIMEOUT: Duration = Duration::from_millis(500);
const RUNTIME_EVENT_MAX_BYTES: usize = 1024 * 1024;
const NATIVE_PROMPT_RENDER_TIMEOUT: Duration = Duration::from_secs(2);
const NATIVE_PROMPT_POLL_INTERVAL: Duration = Duration::from_millis(20);
const NATIVE_PROMPT_POST_RENDER_DELAY: Duration = Duration::from_millis(80);

pub(crate) fn external_command_path(path: &std::path::Path) -> PathBuf {
    let raw = path.to_string_lossy();
    if let Some(rest) = raw.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{rest}"))
    } else if let Some(rest) = raw.strip_prefix(r"\\?\") {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
}

fn hidden_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}


pub trait AgentRuntimeAdapter: Send + Sync {
    fn kind(&self) -> AgentRuntimeKind;
    fn execute(&self, request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult>;
}

pub struct RuntimeRequest {
    pub run_id: String,
    pub cwd: PathBuf,
    pub project: MultiAgentProject,
    pub task: MultiAgentTask,
    pub agent: AgentProfile,
    pub dependency_artifacts: Vec<ArtifactRecord>,
    pub resume_locator: Option<serde_json::Value>,
    pub user_instructions: Vec<String>,
    pub shared_memories: Vec<MemoryEntry>,
    pub collaboration_messages: Vec<CollaborationMessage>,
    pub collaboration: CollaborationAccess,
    pub store: MultiAgentStore,
    pub terminal_host: AgentTerminalHost,
}

pub struct RuntimeResult {
    pub summary: String,
    pub session_locator: serde_json::Value,
}

type TerminalEnvironment = Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>;

#[derive(Clone)]
pub struct AgentTerminalHost {
    pty: PtyManager,
    sessions: SessionManager,
    environment_ready: Arc<Mutex<HashMap<String, TerminalEnvironment>>>,
    native_runtimes: Arc<Mutex<HashMap<String, NativeRuntimeBinding>>>,
}

#[derive(Debug, Clone)]
struct NativeRuntimeBinding {
    kind: AgentRuntimeKind,
    model: Option<String>,
    cwd: PathBuf,
    pty_id: u32,
    environment: TerminalEnvironment,
    program: std::ffi::OsString,
    arguments: Vec<std::ffi::OsString>,
}

impl NativeRuntimeBinding {
    fn matches_command(&self, command: &Command) -> bool {
        self.program == command.get_program()
            && self.arguments.iter().map(|value| value.as_os_str()).eq(command.get_args())
            && self.environment.iter().map(|(key, value)| (key.as_os_str(), value.as_deref())).eq(command.get_envs())
    }

    fn matches_profile(
        &self,
        kind: AgentRuntimeKind,
        cwd: &std::path::Path,
        model: Option<&str>,
    ) -> bool {
        self.kind == kind && self.cwd == cwd && self.model.as_deref() == model
    }
}

// Retain receipt/processing evidence but never revive a finished or cancelled task input.
struct PiTaskDeliveryGuard {
    sessions: SessionManager,
    session_id: String,
    message_id: String,
}
impl Drop for PiTaskDeliveryGuard {
    fn drop(&mut self) {
        if let Err(error) = self
            .sessions
            .retire_runtime_task_delivery(&self.session_id, &self.message_id)
        {
            eprintln!("[CrewVela] 标记 Pi 任务收件结束失败: {error:#}");
        }
    }
}

impl AgentTerminalHost {
    pub fn new(pty: PtyManager, sessions: SessionManager) -> Self {
        Self {
            pty,
            sessions,
            environment_ready: Arc::new(Mutex::new(HashMap::new())),
            native_runtimes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn prepare_project_sessions(&self, store: &MultiAgentStore, project_id: &str) -> Result<()> {
        let _lifecycle = store.agent_lifecycle_guard();
        let snapshot = store.snapshot(project_id)?;
        for agent in snapshot
            .agents
            .iter()
            .filter(|agent| agent.enabled && agent.runtime_kind.ensure_available().is_ok())
        {
            let session = self.sessions.create_agent(NewAgentSession {
                avatar_id: agent.avatar_id.clone(),
                project_id: project_id.to_string(),
                agent_id: agent.id.clone(),
                runtime_kind: agent.runtime_kind.as_str().to_string(),
                agent_name: agent.name.clone(),
                cwd: snapshot.project.root_path.clone(),
            })?;
            self.sessions
                .mark_agent_activity(&session.id, SessionStatus::Pending, None)?;
            store.link_agent_app_session(project_id, &agent.id, &session.id)?;
            if !session
                .pty_id
                .is_some_and(|pty_id| self.pty.is_running(pty_id))
            {
                // Root exit does not prove descendants stopped. Check any
                // tracked PTY before migrating the session's execution mode.
                if let Some(id) = session.pty_id {
                    self.pty.kill(id).map_err(|error| anyhow!(UnverifiedStop(error.to_string())))?;
                }
                self.sessions
                    .migrate_inactive_runtime_command_host(&session.id, |id| {
                        self.pty.is_running(id)
                    })?;
            }
        }
        Ok(())
    }

    fn ensure_session(
        &self,
        request: &RuntimeRequest,
        command: &Command,
    ) -> Result<(SessionRecord, u32)> {
        let mut session = self.sessions.create_agent(NewAgentSession {
            avatar_id: request.agent.avatar_id.clone(),
            project_id: request.project.id.clone(),
            agent_id: request.agent.id.clone(),
            runtime_kind: request.agent.runtime_kind.as_str().to_string(),
            agent_name: request.agent.name.clone(),
            cwd: request.cwd.display().to_string(),
        })?;
        self.sessions.set_runtime_command_host(&session.id, true)?;
        let environment = command.get_envs().map(|(key, value)| (key.to_os_string(), value.map(|value| value.to_os_string()))).collect();
        let pty_id = self.ensure_environment_terminal(&session, &request.cwd, environment)?;
        session = self.sessions.attach_runtime(&session.id, pty_id)?;
        request.store.link_agent_app_session(
            &request.project.id,
            &request.agent.id,
            &session.id,
        )?;
        self.sessions
            .mark_agent_activity(&session.id, SessionStatus::Running, None)?;
        Ok((session, pty_id))
    }

    fn ensure_environment_terminal(&self, session: &SessionRecord, cwd: &std::path::Path, environment: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>) -> Result<u32> {
        let environment_matches = self.environment_ready.lock().unwrap().get(&session.id) == Some(&environment);
        if let Some(id) = session.pty_id.filter(|id| environment_matches && self.pty.is_running(*id)) { return Ok(id); }
        if let Some(id) = session.pty_id {
            self.pty.kill(id).map_err(|error| anyhow!(UnverifiedStop(error.to_string())))?;
        }
        let id = self.pty.open_agent_terminal(Some(&cwd.display().to_string()), &session.id, environment.clone())?;
        self.environment_ready.lock().unwrap().insert(session.id.clone(), environment);
        Ok(id)
    }

    fn ensure_native_session(
        &self,
        kind: AgentRuntimeKind,
        request: &RuntimeRequest,
        command: &Command,
        auto_confirm_workspace_trust: bool,
    ) -> Result<(SessionRecord, u32, bool)> {
        let mut session = self.sessions.create_agent(NewAgentSession {
            avatar_id: request.agent.avatar_id.clone(),
            project_id: request.project.id.clone(),
            agent_id: request.agent.id.clone(),
            runtime_kind: request.agent.runtime_kind.as_str().to_string(),
            agent_name: request.agent.name.clone(),
            cwd: request.cwd.display().to_string(),
        })?;
        let environment: TerminalEnvironment = command.get_envs().map(|(key, value)| (key.to_os_string(), value.map(|value| value.to_os_string()))).collect();
        let reusable = self
            .native_runtimes
            .lock()
            .unwrap()
            .get(&session.id)
            .is_some_and(|binding| {
                binding.matches_profile(kind, &request.cwd, None)
                    && binding.matches_command(command)
                    && binding.pty_id == session.pty_id.unwrap_or_default()
                    && self.pty.is_running(binding.pty_id)
            });
        // 用户可直接在原生终端输入，未必有 ma_run；模型变更不能打断这种工作。
        let model_changed_while_busy = self
            .native_runtimes
            .lock()
            .unwrap()
            .get(&session.id)
            .is_some_and(|binding| {
                (binding.model != request.agent.model || !binding.matches_command(command))
                    && session.pty_id == Some(binding.pty_id)
                    && self.pty.is_running(binding.pty_id)
                    && !self.sessions.is_ready_for_prompt(&session.id)
            });
        if model_changed_while_busy {
            return Err(anyhow!(
                "成员终端正在工作，新的模型或环境配置需等待终端空闲后再启动；原会话已保留"
            ));
        }
        let (pty_id, started) = if reusable {
            self.sessions.set_runtime_command_host(&session.id, false)?;
            (session.pty_id.context("原生 Agent 会话缺少 PTY")?, false)
        } else {
            let _starting = self.sessions.begin_runtime_start(&session.id)?;
            if let Some(id) = session.pty_id {
                self.pty.kill(id).map_err(|error| anyhow!(UnverifiedStop(error.to_string())))?;
            }
            self.environment_ready.lock().unwrap().remove(&session.id);
            self.native_runtimes.lock().unwrap().remove(&session.id);
            let mut native_command = Command::new(command.get_program());
            native_command.args(command.get_args());
            for (key, value) in command.get_envs() {
                if let Some(value) = value {
                    native_command.env(key, value);
                } else {
                    native_command.env_remove(key);
                }
            }
            if let Some(cwd) = command.get_current_dir() {
                native_command.current_dir(cwd);
            }
            if kind == AgentRuntimeKind::Pi {
                self.sessions
                    .configure_pi_readiness(&mut native_command, &session.id)?;
            }
            let id = self.pty.open_agent_program(
                &native_command,
                &session.id,
                auto_confirm_workspace_trust,
            )?;
            session = match self.sessions.attach_runtime(&session.id, id) {
                Ok(session) => session,
                Err(error) => {
                    self.pty.kill(id).map_err(|error| anyhow!(UnverifiedStop(error.to_string())))?;
                    return Err(error);
                }
            };
            self.native_runtimes.lock().unwrap().insert(
                session.id.clone(),
                NativeRuntimeBinding {
                    kind,
                    model: request.agent.model.clone(),
                    cwd: request.cwd.clone(),
                    pty_id: id,
                    environment,
                    program: command.get_program().to_os_string(),
                    arguments: command.get_args().map(|value| value.to_os_string()).collect(),
                },
            );
            (id, true)
        };
        request.store.link_agent_app_session(
            &request.project.id,
            &request.agent.id,
            &session.id,
        )?;
        Ok((session, pty_id, started))
    }

    fn execute_native(
        &self,
        kind: AgentRuntimeKind,
        command: &Command,
        prompt: &str,
        request: &RuntimeRequest,
        cancelled: &AtomicBool,
        auto_confirm_workspace_trust: bool,
    ) -> Result<RuntimeResult> {
        let (mut session, mut pty_id, mut started) =
            self.ensure_native_session(kind, request, command, auto_confirm_workspace_trust)?;
        if let Err(error) = self.wait_until_native_ready(
            kind,
            &session,
            pty_id,
            cancelled,
            started,
            &request.store,
            &request.project.id,
        ) {
            if !missing_pty_session_error(&error) {
                return Err(error);
            }
            self.native_runtimes.lock().unwrap().remove(&session.id);
            let reopened =
                self.ensure_native_session(kind, request, command, auto_confirm_workspace_trust)?;
            session = reopened.0;
            pty_id = reopened.1;
            started = reopened.2;
            if let Err(error) = self.wait_until_native_ready(
                kind,
                &session,
                pty_id,
                cancelled,
                started,
                &request.store,
                &request.project.id,
            ) {
                self.stop_native_runtime(
                    &session,
                    pty_id,
                    request.store.handoff_in_progress(&request.project.id)?,
                )?;
                return Err(error);
            }
        }

        self.sessions.ingest_events()?;
        let baseline_revision = self.sessions.agent_turn_revision(&session.id);
        request.store.bind_pty_execution(&request.run_id, pty_id)?;
        let mut cursor = self.pty.transcript_cursor(pty_id)?;
        self.sessions.register_agent_prompt(&session.id, prompt);
        self.sessions
            .mark_agent_activity(&session.id, SessionStatus::Running, None)?;
        let pi_delivery = (kind == AgentRuntimeKind::Pi).then(|| PiTaskDeliveryGuard {
            sessions: self.sessions.clone(),
            session_id: session.id.clone(),
            message_id: format!("task-{}", uuid::Uuid::new_v4()),
        });
        let send_result = if let Some(delivery) = pi_delivery.as_ref() {
            self.sessions
                .queue_runtime_delivery(&session.id, &delivery.message_id, prompt)
                .and_then(|queued| {
                    if queued {
                        Ok(())
                    } else {
                        Err(anyhow!("Pi 原生会话收件桥未就绪，任务未提交"))
                    }
                })
        } else {
            submit_native_prompt(
                prompt,
                |bytes| self.pty.write(pty_id, bytes),
                || self.wait_until_native_prompt_rendered(pty_id, cursor, prompt, cancelled),
                || {
                    if cancelled.load(Ordering::Acquire) {
                        Err(anyhow!("Agent 任务已取消"))
                    } else {
                        Ok(())
                    }
                },
            )
        };
        if let Err(error) = send_result {
            self.sessions.clear_agent_prompt(&session.id);
            self.stop_native_runtime(
                &session,
                pty_id,
                request.store.handoff_in_progress(&request.project.id)?,
            )?;
            return Err(error).context("向原生 Agent 会话发送任务失败");
        }

        let started_at = Instant::now();
        let mut capture = RuntimeStreamCapture::default();
        let mut codex_completion_fallback = CodexNativeCompletionFallback::default();
        loop {
            if cancelled.load(Ordering::Acquire) {
                self.stop_native_runtime(
                    &session,
                    pty_id,
                    request.store.handoff_in_progress(&request.project.id)?,
                )?;
                return Err(anyhow!("Agent 任务已取消"));
            }
            self.sessions.ingest_events()?;
            if let Some(delivery) = pi_delivery.as_ref() {
                if let Some(error) = self
                    .sessions
                    .runtime_delivery_error(&session.id, &delivery.message_id)
                {
                    return Err(anyhow!(error));
                }
            }
            let (next_cursor, bytes) = self.pty.transcript_since(pty_id, cursor)?;
            cursor = next_cursor;
            let transcript_changed = !bytes.is_empty();
            if !bytes.is_empty() {
                capture.append(&bytes);
            }
            if let Some(hook_summary) =
                self.sessions
                    .agent_turn_summary_after(&session.id, baseline_revision, prompt)
            {
                if kind == AgentRuntimeKind::Pi && hook_summary.trim().is_empty() {
                    return Err(anyhow!(self
                        .sessions
                        .get(&session.id)
                        .and_then(|current| current.last_error)
                        .unwrap_or_else(|| "Pi 本轮未返回可用结果".to_string())));
                }
                let summary = if hook_summary.trim().is_empty() {
                    terminal_output_summary(&capture.output())
                } else {
                    hook_summary
                };
                if summary.trim().is_empty() {
                    return Err(anyhow!("{} 未返回可用结果", request.agent.name));
                }
                let current = self.sessions.get(&session.id).unwrap_or(session.clone());
                let mut locator = serde_json::json!({
                    "ptyId": pty_id,
                    "cwd": request.cwd,
                    "adapter": kind,
                });
                if let Some(object) = locator.as_object_mut() {
                    object.insert(
                        if kind == AgentRuntimeKind::Codex {
                            "threadId".to_string()
                        } else {
                            "sessionId".to_string()
                        },
                        serde_json::Value::String(current.claude_session_id),
                    );
                }
                return Ok(RuntimeResult {
                    summary,
                    session_locator: locator,
                });
            }
            if kind == AgentRuntimeKind::Codex {
                let transcript = capture.output();
                if codex_completion_fallback.observe(
                    &transcript,
                    transcript_changed,
                    Instant::now(),
                ) {
                    if let Some(error) = codex_protocol_failure(&transcript)
                        .or_else(|| codex_known_runtime_failure(&transcript))
                    {
                        self.sessions.clear_agent_prompt(&session.id);
                        return Err(anyhow!("{} 执行失败：{error}", request.agent.name));
                    }
                    let summary = terminal_output_summary(&transcript);
                    if summary.trim().is_empty() {
                        return Err(anyhow!("{} 未返回可用结果", request.agent.name));
                    }
                    self.sessions
                        .complete_agent_turn_fallback(&session.id, prompt, &summary)?;
                    let current = self.sessions.get(&session.id).unwrap_or(session.clone());
                    let mut locator = serde_json::json!({
                        "ptyId": pty_id,
                        "cwd": request.cwd,
                        "adapter": kind,
                    });
                    if let Some(object) = locator.as_object_mut() {
                        object.insert(
                            "threadId".to_string(),
                            serde_json::Value::String(current.claude_session_id),
                        );
                    }
                    if !self.pty.is_running(pty_id) { self.stop_native_runtime(&session, pty_id, false)?; }
                    return Ok(RuntimeResult {
                        summary,
                        session_locator: locator,
                    });
                }
            }
            if !self.pty.is_running(pty_id) {
                self.stop_native_runtime(&session, pty_id, false)?;
                return Err(anyhow!("{} 原生会话意外退出", request.agent.name));
            }
            if started_at.elapsed() >= RUN_TIMEOUT {
                self.stop_native_runtime(
                    &session,
                    pty_id,
                    request.store.handoff_in_progress(&request.project.id)?,
                )?;
                return Err(anyhow!("{} 执行超时", request.agent.name));
            }
            std::thread::sleep(Duration::from_millis(80));
        }
    }

    fn wait_until_native_ready(
        &self,
        kind: AgentRuntimeKind,
        session: &SessionRecord,
        pty_id: u32,
        cancelled: &AtomicBool,
        freshly_started: bool,
        store: &MultiAgentStore,
        project_id: &str,
    ) -> Result<()> {
        let started = Instant::now();
        let mut cursor = 0;
        let mut startup_capture = RuntimeStreamCapture::default();
        let mut codex_update_skipped = false;
        loop {
            if cancelled.load(Ordering::Acquire) {
                self.stop_native_runtime(session, pty_id, store.handoff_in_progress(project_id)?)?;
                return Err(anyhow!("Agent 任务已取消"));
            }
            self.sessions.ingest_events()?;
            if kind == AgentRuntimeKind::Codex && freshly_started {
                let (next_cursor, bytes) = self.pty.transcript_since(pty_id, cursor)?;
                cursor = next_cursor;
                startup_capture.append(&bytes);
            }
            if kind == AgentRuntimeKind::Codex
                && freshly_started
                && !codex_update_skipped
                && codex_native_update_prompt(&startup_capture.output())
            {
                self.pty.write(pty_id, b"\x1b[B\r")?;
                codex_update_skipped = true;
            }
            let ready = match kind {
                AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Pi => {
                    self.sessions.is_ready_for_prompt(&session.id)
                }
                AgentRuntimeKind::Codex => {
                    self.sessions.is_ready_for_prompt(&session.id)
                        || (freshly_started && codex_native_prompt_ready(&startup_capture.output()))
                }
                _ => true,
            };
            if ready {
                return Ok(());
            }
            if !self.pty.is_running(pty_id) {
                self.stop_native_runtime(session, pty_id, false)?;
                return Err(anyhow!("{} 原生会话启动失败", session.title));
            }
            if started.elapsed() >= Duration::from_secs(45) {
                return Err(anyhow!(
                    "{} 原生会话尚未就绪，请在可视终端中处理登录或权限提示",
                    session.title
                ));
            }
            std::thread::sleep(Duration::from_millis(80));
        }
    }

    fn wait_until_native_prompt_rendered(
        &self,
        pty_id: u32,
        mut cursor: u64,
        prompt: &str,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        let started = Instant::now();
        let mut capture = RuntimeStreamCapture::default();
        loop {
            if cancelled.load(Ordering::Acquire) {
                return Err(anyhow!("Agent 任务已取消"));
            }
            let (next_cursor, bytes) = self.pty.transcript_since(pty_id, cursor)?;
            cursor = next_cursor;
            if !bytes.is_empty() {
                capture.append(&bytes);
                if native_prompt_paste_rendered(prompt, &capture.output()) {
                    std::thread::sleep(NATIVE_PROMPT_POST_RENDER_DELAY);
                    return Ok(());
                }
            }
            if !self.pty.is_running(pty_id) {
                return Err(anyhow!("原生 Agent 会话在提交 Prompt 前已退出"));
            }
            if started.elapsed() >= NATIVE_PROMPT_RENDER_TIMEOUT {
                return Ok(());
            }
            std::thread::sleep(NATIVE_PROMPT_POLL_INTERVAL);
        }
    }

    fn stop_native_runtime(
        &self,
        session: &SessionRecord,
        pty_id: u32,
        strict: bool,
    ) -> Result<()> {
        let stopped = if strict {
            self.pty.stop_for_handoff(pty_id)
        } else {
            let _ = self.pty.write(pty_id, &[3]);
            std::thread::sleep(Duration::from_millis(150));
            self.pty.kill(pty_id)
        };
        self.finish_native_stop(session, stopped)
    }

    fn finish_native_stop(&self, session: &SessionRecord, stopped: Result<()>) -> Result<()> {
        stopped.map_err(|error| anyhow!(UnverifiedStop(error.to_string())))?;
        self.native_runtimes.lock().unwrap().remove(&session.id);
        self.sessions.mark_stopped(&session.id)?;
        Ok(())
    }

    fn execute(
        &self,
        kind: AgentRuntimeKind,
        command: &Command,
        stdin_payload: Option<&str>,
        request: &RuntimeRequest,
        cancelled: &AtomicBool,
    ) -> Result<VisibleRuntimeOutput> {
        let (session, pty_id) = self.ensure_session(request, command)?;
        request.store.bind_pty_execution(&request.run_id, pty_id)?;
        let marker = format!("__CREWVELA_RUN_DONE_{}__:", request.run_id.replace('-', "_"));
        let runtime_files =
            VisibleRuntimeFiles::create(kind, &request.run_id, command, stdin_payload, &marker)?;

        let mut cursor = self.pty.transcript_cursor(pty_id)?;
        let launch = if cfg!(windows) { format!(
            "powershell.exe -NoProfile -ExecutionPolicy Bypass -File {}\r",
            powershell_quote(&runtime_files.script_path.display().to_string())
        ) } else { format!("set +m; /bin/bash {}\r", posix_quote(&runtime_files.script_path.display().to_string())) };
        self.pty.write(pty_id, launch.as_bytes())?;

        let started = Instant::now();
        let mut capture = RuntimeStreamCapture::default();
        let mut marker_window = VecDeque::new();
        let outcome = loop {
            if cancelled.load(Ordering::Acquire) {
                self.interrupt_visible_runtime(
                    &session,
                    pty_id,
                    cursor,
                    &marker,
                    request.store.handoff_in_progress(&request.project.id)?,
                )?;
                break Err(anyhow!("Agent 任务已取消"));
            }
            let (next_cursor, bytes) = self.pty.transcript_since(pty_id, cursor)?;
            cursor = next_cursor;
            if !bytes.is_empty() {
                capture.append(&bytes);
                marker_window.extend(bytes.iter().copied());
                let keep = marker.len().saturating_add(64);
                if marker_window.len() > keep {
                    marker_window.drain(..marker_window.len() - keep);
                }
            }
            let marker_output = String::from_utf8_lossy(marker_window.make_contiguous());
            if let Some(code) = marker_exit_code(&marker_output, &marker) {
                let output = std::fs::read(&runtime_files.output_path)
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                    .unwrap_or_else(|_| capture.output());
                break Ok(VisibleRuntimeOutput {
                    pty_id,
                    output,
                    succeeded: code == 0,
                });
            }
            if let Ok(code_text) = std::fs::read_to_string(&runtime_files.exit_sentinel_path) {
                if let Ok(code) = code_text.trim().parse::<i32>() {
                    let output = std::fs::read(&runtime_files.output_path)
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
                        .unwrap_or_else(|_| capture.output());
                    break Ok(VisibleRuntimeOutput {
                        pty_id,
                        output,
                        succeeded: code == 0,
                    });
                }
            }
            if kind == AgentRuntimeKind::Codex {
                if let Ok(output) = std::fs::read_to_string(&runtime_files.output_path) {
                    if codex_protocol_turn_completed(&output) {
                        break Ok(VisibleRuntimeOutput {
                            pty_id,
                            output,
                            succeeded: true,
                        });
                    }
                }
            }
            if !self.pty.is_running(pty_id) {
                self.pty.kill(pty_id).map_err(|error| anyhow!(UnverifiedStop(error.to_string())))?;
                self.environment_ready.lock().unwrap().remove(&session.id);
                break Err(anyhow!("{} 终端已退出，未收到完整完成凭据", request.agent.name));
            }
            if started.elapsed() >= RUN_TIMEOUT {
                self.interrupt_visible_runtime(
                    &session,
                    pty_id,
                    cursor,
                    &marker,
                    request.store.handoff_in_progress(&request.project.id)?,
                )?;
                break Err(anyhow!("{} 执行超时", request.agent.name));
            }
            std::thread::sleep(Duration::from_millis(80));
        };

        if !self.pty.is_running(pty_id) {
            self.pty.kill(pty_id).map_err(|error| anyhow!(UnverifiedStop(error.to_string())))?;
            self.environment_ready.lock().unwrap().remove(&session.id);
        }
        match &outcome {
            Ok(_) => {
                let _ =
                    self.sessions
                        .mark_agent_activity(&session.id, SessionStatus::Completed, None);
            }
            Err(error) => {
                let session_status = if error.downcast_ref::<UnverifiedStop>().is_some() { SessionStatus::Error } else if cancelled.load(Ordering::Acquire)
                    && request
                        .store
                        .project(&request.project.id)
                        .is_ok_and(|project| project.status == ProjectStatus::Paused)
                {
                    SessionStatus::Stopped
                } else {
                    SessionStatus::Error
                };
                let _ = self.sessions.mark_agent_activity(
                    &session.id,
                    session_status,
                    Some(error.to_string()),
                );
            }
        }
        let output = outcome?;
        if let Some(log) = present_runtime_log(kind, &output.output, "stdout") {
            let _ = request
                .store
                .append_run_log(&request.run_id, "stdout", &log);
        }
        Ok(output)
    }

    fn interrupt_visible_runtime(
        &self,
        session: &SessionRecord,
        pty_id: u32,
        mut cursor: u64,
        marker: &str,
        strict: bool,
    ) -> Result<()> {
        let _ = self.pty.write(pty_id, &[3]);
        let started = Instant::now();
        let mut marker_window = VecDeque::new();
        while started.elapsed() < VISIBLE_RUNTIME_INTERRUPT_TIMEOUT {
            match self.pty.transcript_since(pty_id, cursor) {
                Ok((next_cursor, bytes)) => {
                    cursor = next_cursor;
                    marker_window.extend(bytes);
                    let keep = marker.len().saturating_add(64);
                    if marker_window.len() > keep {
                        marker_window.drain(..marker_window.len() - keep);
                    }
                    let output = String::from_utf8_lossy(marker_window.make_contiguous());
                    if marker_exit_code(&output, marker).is_some() {
                        return Ok(());
                    }
                }
                Err(_) => break,
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        if strict {
            self.pty
                .stop_for_handoff(pty_id)
                .map_err(|error| anyhow!(UnverifiedStop(error.to_string())))?;
        } else {
            self.pty.kill(pty_id).map_err(|error| anyhow!(UnverifiedStop(error.to_string())))?;
        }
        self.environment_ready.lock().unwrap().remove(&session.id);
        Ok(())
    }
}

struct VisibleRuntimeOutput {
    pty_id: u32,
    output: String,
    succeeded: bool,
}

struct VisibleRuntimeFiles {
    script_path: PathBuf,
    prompt_path: Option<PathBuf>,
    output_path: PathBuf,
    exit_sentinel_path: PathBuf,
}

impl VisibleRuntimeFiles {
    fn create(
        kind: AgentRuntimeKind,
        run_id: &str,
        command: &Command,
        stdin_payload: Option<&str>,
        marker: &str,
    ) -> Result<Self> {
        let runtime_dir = runtime_prompt_directory()?.join("visible");
        std::fs::create_dir_all(&runtime_dir)?;
        let prompt_path = stdin_payload
            .map(|payload| {
                let path = runtime_dir.join(format!("{run_id}.stdin.txt"));
                std::fs::write(&path, payload.as_bytes()).map(|_| path)
            })
            .transpose()?;
        let output_path = runtime_dir.join(format!("{run_id}.output.jsonl"));
        let exit_sentinel_path = runtime_dir.join(format!("{run_id}.exitcode"));
        let script_path = runtime_dir.join(format!("{run_id}.{}", if cfg!(windows) { "ps1" } else { "sh" }));
        let script = if cfg!(windows) { powershell_runtime_script(
            kind,
            command,
            prompt_path.as_deref(),
            &output_path,
            &exit_sentinel_path,
            marker,
        ) } else { posix_runtime_script(command, prompt_path.as_deref(), &output_path, &exit_sentinel_path, marker) };
        let mut encoded = if cfg!(windows) { vec![0xEF, 0xBB, 0xBF] } else { Vec::new() };
        encoded.extend_from_slice(script.as_bytes());
        if let Err(error) = std::fs::write(&script_path, encoded) {
            if let Some(path) = &prompt_path {
                let _ = std::fs::remove_file(path);
            }
            return Err(error).with_context(|| {
                format!("写入可视 Agent 启动脚本失败：{}", script_path.display())
            });
        }
        Ok(Self {
            script_path,
            prompt_path,
            output_path,
            exit_sentinel_path,
        })
    }
}

impl Drop for VisibleRuntimeFiles {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.script_path);
        if let Some(path) = &self.prompt_path {
            let _ = std::fs::remove_file(path);
        }
        let _ = std::fs::remove_file(&self.output_path);
        let _ = std::fs::remove_file(&self.exit_sentinel_path);
    }
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn posix_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn posix_runtime_script(command: &Command, prompt_path: Option<&std::path::Path>, output_path: &std::path::Path, exit_sentinel_path: &std::path::Path, marker: &str) -> String {
    let mut script = String::from("#!/bin/bash\nset -o pipefail\n");
    script.push_str(&format!("cc_finish() {{ cc_status=$?; printf '%s' \"$cc_status\" > {}; printf '\\n%s%s\\n' {} \"$cc_status\"; }}\ntrap cc_finish EXIT\ntrap 'exit 130' INT\ntrap 'exit 143' TERM\n", posix_quote(&exit_sentinel_path.to_string_lossy()), posix_quote(marker)));
    if let Some(cwd) = command.get_current_dir() {
        script.push_str(&format!("cd -- {} || exit 1\n", posix_quote(&cwd.to_string_lossy())));
    }
    script.push_str(&posix_quote(&command.get_program().to_string_lossy()));
    for argument in command.get_args() { script.push(' '); script.push_str(&posix_quote(&argument.to_string_lossy())); }
    if let Some(path) = prompt_path { script.push_str(&format!(" < {}", posix_quote(&path.to_string_lossy()))); }
    // Bound tee's output file with the kernel's RLIMIT_FSIZE. Bash uses 1024
    // byte blocks here (POSIX-mode shells can use 512, which is stricter).
    // Limit only the collector, never files written by the Agent itself.
    script.push_str(&format!(" 2>&1 | (ulimit -f 5120 || exit 74; exec tee {})\ncc_codes=(\"${{PIPESTATUS[@]}}\")\ncc_status=${{cc_codes[0]}}\nif [ \"${{cc_codes[1]}}\" -ne 0 ]; then printf '%s\\n' 'Agent 输出收集失败或超过 5 MiB 上限，结果不完整' >&2; cc_status=74; fi\nexit \"$cc_status\"\n", posix_quote(&output_path.to_string_lossy())));
    script
}

fn powershell_runtime_script(
    kind: AgentRuntimeKind,
    command: &Command,
    prompt_path: Option<&std::path::Path>,
    output_path: &std::path::Path,
    exit_sentinel_path: &std::path::Path,
    marker: &str,
) -> String {
    let mut lines = vec![
        "$ErrorActionPreference = 'Continue'".to_string(),
        "$OutputEncoding = [System.Text.UTF8Encoding]::new($false)".to_string(),
        "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)".to_string(),
        "$crewvelaRawEncoding = [System.Text.UTF8Encoding]::new($false)".to_string(),
        format!(
            "$crewvelaRawOutput = {}",
            powershell_quote(&output_path.display().to_string())
        ),
        "$crewvelaRawTrimAt = 5242880".to_string(),
        "$crewvelaRawPrefixBytes = 262144".to_string(),
        "$crewvelaRawTailBytes = 4194304".to_string(),
        "$crewvelaRawLinePrefixChars = 65536".to_string(),
        "$crewvelaRawLineTailChars = 1048576".to_string(),
        "[System.IO.File]::WriteAllText($crewvelaRawOutput, '', $crewvelaRawEncoding)".to_string(),
    ];
    if let Some(cwd) = command.get_current_dir() {
        lines.push(format!(
            "Set-Location -LiteralPath {}",
            powershell_quote(&cwd.display().to_string())
        ));
    }
    let executable = powershell_quote(&command.get_program().to_string_lossy());
    let arguments = command
        .get_args()
        .map(|argument| powershell_quote(&argument.to_string_lossy()))
        .collect::<Vec<_>>()
        .join(" ");
    let invocation = match prompt_path {
        Some(path) => format!(
            "Get-Content -Raw -Encoding UTF8 -LiteralPath {} | & {executable} {arguments}",
            powershell_quote(&path.display().to_string())
        ),
        None => format!("& {executable} {arguments}"),
    };
    lines.extend(powershell_protocol_presenter(kind));
    lines.push(format!("{invocation} 2>&1 | ForEach-Object {{"));
    lines.push("  $crewvelaLine = $_.ToString()".into());
    lines.push("  $crewvelaPersistedLine = if ($crewvelaLine.Length -gt ($crewvelaRawLinePrefixChars + $crewvelaRawLineTailChars)) { $crewvelaLine.Substring(0, $crewvelaRawLinePrefixChars) + $crewvelaLine.Substring($crewvelaLine.Length - $crewvelaRawLineTailChars) } else { $crewvelaLine }".into());
    lines.push("  [System.IO.File]::AppendAllText($crewvelaRawOutput, $crewvelaPersistedLine + [Environment]::NewLine, $crewvelaRawEncoding)".into());
    lines.push(
        "  if ((Get-Item -LiteralPath $crewvelaRawOutput).Length -gt $crewvelaRawTrimAt) {".into(),
    );
    lines.push("    $crewvelaRawBytes = [System.IO.File]::ReadAllBytes($crewvelaRawOutput)".into());
    lines.push(
        "    $crewvelaBounded = New-Object byte[] ($crewvelaRawPrefixBytes + $crewvelaRawTailBytes)"
            .into(),
    );
    lines.push(
        "    [Array]::Copy($crewvelaRawBytes, 0, $crewvelaBounded, 0, $crewvelaRawPrefixBytes)".into(),
    );
    lines.push("    [Array]::Copy($crewvelaRawBytes, $crewvelaRawBytes.Length - $crewvelaRawTailBytes, $crewvelaBounded, $crewvelaRawPrefixBytes, $crewvelaRawTailBytes)".into());
    lines.push("    [System.IO.File]::WriteAllBytes($crewvelaRawOutput, $crewvelaBounded)".into());
    lines.push("  }".into());
    lines.push("  Write-CrewVelaProtocolEvent $crewvelaLine".into());
    lines.push("}".into());
    lines.push("$crewvelaInvocationSucceeded = $?".into());
    lines.push("$crewvelaExitCode = if ($null -eq $LASTEXITCODE) { if ($crewvelaInvocationSucceeded) { 0 } else { 1 } } else { $LASTEXITCODE }".into());
    // 退出码落盘作为哨兵：转录文本里的完成标记可能因终端换行/回滚丢失，
    // 哨兵文件是独立于转录通道的收尾保证。
    lines.push(format!(
        "[System.IO.File]::WriteAllText({}, \"$crewvelaExitCode\", $crewvelaRawEncoding)",
        powershell_quote(&exit_sentinel_path.display().to_string())
    ));
    lines.push(format!(
        "[Console]::Out.WriteLine({} + $crewvelaExitCode)",
        powershell_quote(marker)
    ));
    lines.join("\r\n")
}

fn powershell_protocol_presenter(kind: AgentRuntimeKind) -> Vec<String> {
    let label = match kind {
        AgentRuntimeKind::ClaudeCode => "Claude Code",
        AgentRuntimeKind::Codex => "Codex",
        AgentRuntimeKind::KimiCode => "Kimi Code",
        AgentRuntimeKind::MiniMaxCode | AgentRuntimeKind::Unsupported => "已停用的 CLI",
        AgentRuntimeKind::Pi => "Pi",
        AgentRuntimeKind::OhMyPi => "Oh My Pi（已移除）",
        AgentRuntimeKind::OpenCode => "OpenCode",
        AgentRuntimeKind::DeepSeekHarness => "DeepSeek Harness",
        AgentRuntimeKind::ClaudeChat => "Claude Code Chat",
        AgentRuntimeKind::KimiChat => "Kimi Code Chat",
        AgentRuntimeKind::PiChat => "Pi Chat",
        AgentRuntimeKind::CodexChat => "Codex Chat",
        AgentRuntimeKind::OpenCodeChat => "OpenCode Chat",
        AgentRuntimeKind::ZCode => "ZCode Desktop",
        AgentRuntimeKind::ZCodeCli => "ZCode CLI",
    };
    vec![
        "$crewvelaThinkingShown = $false".into(),
        "function Write-CrewVelaProtocolEvent([string]$line) {".into(),
        "  try { $event = $line | ConvertFrom-Json -ErrorAction Stop } catch { Write-Host $line; return }".into(),
        "  $type = [string]$event.type".into(),
        "  $subtype = [string]$event.subtype".into(),
        format!("  $runtimeLabel = {}", powershell_quote(label)),
        "  if ($type -eq 'delta' -and $event.content) { Write-Host -NoNewline ([string]$event.content); return }".into(),
        "  if ($type -eq 'exec.result') { if ($event.status -eq 'succeeded') { Write-Host ''; Write-Host ('✓ ' + $runtimeLabel + ' 本轮执行完成') } else { Write-Host ('✗ ' + $runtimeLabel + ' 执行失败') }; return }".into(),
        "  if ($type -eq 'message' -and [string]$event.message.role -eq 'assistant') { return }".into(),
        "  if ($type -eq 'system' -and $subtype -eq 'init') { Write-Host ('◆ ' + $runtimeLabel + ' 已启动' + $(if ($event.model) { ' · ' + $event.model } else { '' })); return }".into(),
        "  if ($type -eq 'system' -and $subtype -eq 'thinking_tokens') { if (-not $script:crewvelaThinkingShown) { Write-Host ('… ' + $runtimeLabel + ' 正在思考'); $script:crewvelaThinkingShown = $true }; return }".into(),
        "  if ($type -eq 'system') { return }".into(),
        "  if (($type -eq 'item.started' -or $type -eq 'item.completed') -and ([string]$event.item.type -match 'reasoning|thinking|analysis')) { return }".into(),
        "  if (($type -eq 'item.started' -or $type -eq 'item.completed') -and $event.item.content) { foreach ($content in @($event.item.content)) { if ($content.text) { Write-Host $content.text } elseif ($content.message) { Write-Host $content.message } }; return }".into(),
        "  if ($type -eq 'message_end' -and [string]$event.message.role -eq 'assistant') { foreach ($content in @($event.message.content)) { if ($content.text) { Write-Host $content.text } }; return }".into(),
        "  if ($type -eq 'message_start') { return }".into(),
        "  if ($type -eq 'tool_execution_start') { $toolName = if ($event.toolName) { $event.toolName } elseif ($event.name) { $event.name } else { '工具' }; Write-Host ('→ 调用 ' + $toolName); return }".into(),
        "  if ($type -eq 'tool_execution_end') { return }".into(),
        "  if ($type -eq 'agent_end') { Write-Host ('✓ ' + $runtimeLabel + ' 本轮执行完成'); return }".into(),
        "  if ($type -eq 'text' -and $event.text) { Write-Host $event.text; return }".into(),
        "  if ($event.part.text) { Write-Host $event.part.text; return }".into(),
        "  if ($type -eq 'turn.started') { Write-Host ('… ' + $runtimeLabel + ' 正在执行任务'); return }".into(),
        "  if ($type -eq 'turn.completed') { Write-Host ('✓ ' + $runtimeLabel + ' 本轮执行完成'); return }".into(),
        "  if ($type -eq 'turn.failed' -or $type -eq 'error') { $message = if ($event.error.message) { $event.error.message } elseif ($event.message) { $event.message } else { '未返回错误详情' }; Write-Host ('✗ 执行失败：' + $message); return }".into(),
        "  if ($type -eq 'assistant') { foreach ($item in @($event.message.content)) { if ($item.type -eq 'text' -and $item.text) { Write-Host $item.text } elseif ($item.type -eq 'tool_use') { Write-Host ('→ 调用 ' + $item.name) } }; return }".into(),
        "  if ($type -eq 'item.started' -or $type -eq 'item.completed') { $item = $event.item; if ($item.type -eq 'reasoning') { return }; if ($item.text) { Write-Host $item.text } elseif ($item.message) { Write-Host $item.message } elseif ($item.command) { Write-Host ('→ ' + $item.command) }; return }".into(),
        "  if ($type -eq 'result') { if ($event.result) { Write-Host ('✓ ' + $event.result) } elseif ($event.is_error) { Write-Host '✗ Agent 返回失败结果' }; return }".into(),
        "  if ($event.message -is [string] -and $event.message) { Write-Host $event.message; return }".into(),
        "}".into(),
    ]
}

fn marker_exit_code(output: &str, marker: &str) -> Option<i32> {
    let tail = output.rsplit_once(marker)?.1;
    let digits = tail
        .trim_start()
        .chars()
        .take_while(|character| character.is_ascii_digit() || *character == '-')
        .collect::<String>();
    (!digits.is_empty()).then(|| digits.parse().ok()).flatten()
}

fn project_runtime_leases(
    store: &MultiAgentStore,
    project_id: &str,
) -> Result<Vec<crate::runtime_preferences::RuntimeLease>> {
    let snapshot = store.snapshot(project_id)?;
    let mut kinds = Vec::new();
    for agent in snapshot
        .agents
        .iter()
        .filter(|agent| agent.enabled && agent.runtime_kind.ensure_available().is_ok())
    {
        if !kinds.contains(&agent.runtime_kind) {
            kinds.push(agent.runtime_kind);
        }
    }
    kinds
        .into_iter()
        .map(crate::runtime_preferences::acquire)
        .collect()
}

#[derive(Clone, Default)]
pub struct MultiAgentSupervisor {
    run_stops: Arc<Mutex<HashMap<String, (String, Arc<AtomicBool>)>>>,
    projects: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    message_projects: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    shutting_down: Arc<AtomicBool>,
}

pub(crate) struct SupervisorShutdownGuard {
    supervisor: MultiAgentSupervisor,
    pub(crate) project_ids: Vec<String>,
    committed: bool,
}
impl SupervisorShutdownGuard {
    pub(crate) fn commit(mut self) { self.committed = true; }
}
impl Drop for SupervisorShutdownGuard {
    fn drop(&mut self) {
        if !self.committed { self.supervisor.shutting_down.store(false, Ordering::Release); }
    }
}

impl MultiAgentSupervisor {
    fn register_start(&self, project_id: &str, cancelled: &Arc<AtomicBool>, message: bool) -> Result<bool> {
        let registry = if message { &self.message_projects } else { &self.projects };
        let mut projects = registry.lock().unwrap();
        anyhow::ensure!(!self.shutting_down.load(Ordering::Acquire), "应用正在退出，不能启动协作执行或消息投递");
        if let Some(current) = projects.get(project_id) {
            if !message && current.load(Ordering::Acquire) {
                anyhow::bail!("项目调度器正在停止，请等待停止完成后重试启动");
            }
            return Ok(false);
        }
        projects.insert(project_id.to_owned(), Arc::clone(cancelled));
        Ok(true)
    }
    pub(crate) fn begin_shutdown(&self) -> Result<SupervisorShutdownGuard> {
        let projects = self.projects.lock().unwrap();
        let messages = self.message_projects.lock().unwrap();
        anyhow::ensure!(!self.shutting_down.swap(true, Ordering::AcqRel), "应用已在退出，拒绝重复停止批次");
        let mut project_ids = projects.keys().chain(messages.keys()).cloned().collect::<HashSet<_>>().into_iter().collect::<Vec<_>>();
        project_ids.sort();
        Ok(SupervisorShutdownGuard { supervisor:self.clone(), project_ids, committed:false })
    }
    pub fn active_project_ids(&self) -> Vec<String> {
        let projects = self.projects.lock().unwrap();
        let messages = self.message_projects.lock().unwrap();
        let mut ids = projects.keys().chain(messages.keys()).cloned().collect::<HashSet<_>>().into_iter().collect::<Vec<_>>();
        ids.sort();
        ids
    }

    pub fn start(
        &self,
        store: MultiAgentStore,
        project_id: String,
        terminal_host: AgentTerminalHost,
        collaboration_bridge: CollaborationBridge,
    ) -> Result<ProjectExecutionReceipt> {
        let runtime_leases = project_runtime_leases(&store, &project_id)?;
        cleanup_stale_runtime_prompts();
        let cancelled = Arc::new(AtomicBool::new(false));
        {
            if !self.register_start(&project_id, &cancelled, false)? {
                return Ok(ProjectExecutionReceipt {
                    project_id,
                    accepted: true,
                    message: "项目调度器正在启动或运行，已复用现有调度".to_string(),
                });
            }
        }
        // 准备工作在锁外执行，避免 pause/cancel 被阻塞；失败时回滚注册，保证可以重试。
        if let Err(error) = terminal_host.prepare_project_sessions(&store, &project_id) {
            self.projects.lock().unwrap().remove(&project_id);
            return Err(error);
        }
        let supervisor = self.clone();
        let thread_project_id = project_id.clone();
        std::thread::spawn(move || {
            let _runtime_leases = runtime_leases;
            if supervisor.activate_project_if_current(&store, &thread_project_id, &cancelled).unwrap_or_else(|error| {
                eprintln!("[crewvela] 项目启动状态更新失败: {error:#}"); false
            }) {
            let _ = schedule_until_idle(
                &store,
                &thread_project_id,
                cancelled,
                terminal_host,
                collaboration_bridge,
                false,
                supervisor.run_stops.clone(),
            );
            }
            supervisor
                .projects
                .lock()
                .unwrap()
                .remove(&thread_project_id);
        });
        Ok(ProjectExecutionReceipt {
            project_id,
            accepted: true,
            message: "项目调度已启动，任务会按依赖与 Agent 并发限制执行".to_string(),
        })
    }

    fn activate_project_if_current(&self, store: &MultiAgentStore, project_id: &str, cancelled: &Arc<AtomicBool>) -> Result<bool> {
        let projects = self.projects.lock().unwrap();
        if cancelled.load(Ordering::Acquire) || !projects.get(project_id).is_some_and(|current| Arc::ptr_eq(current, cancelled)) { return Ok(false); }
        // Activation and pause share the registration lock and state-write order.
        store.set_project_status(project_id, ProjectStatus::Active)?;
        Ok(true)
    }

    /// 消息轮次共享 Agent 并发限制，但不恢复或完成业务项目。
    pub fn start_message_delivery(
        &self,
        store: MultiAgentStore,
        project_id: String,
        terminal_host: AgentTerminalHost,
        collaboration_bridge: CollaborationBridge,
    ) -> Result<()> {
        let runtime_leases = project_runtime_leases(&store, &project_id)?;
        if !matches!(
            store.project(&project_id)?.status,
            ProjectStatus::Draft | ProjectStatus::Active | ProjectStatus::Completed
        ) {
            return Ok(());
        }
        let cancelled = Arc::new(AtomicBool::new(false));
        {
            if !self.register_start(&project_id, &cancelled, true)? {
                return Ok(());
            }
        }
        let supervisor = self.clone();
        std::thread::spawn(move || {
            let _runtime_leases = runtime_leases;
            let _ = schedule_until_idle(
                &store,
                &project_id,
                cancelled,
                terminal_host,
                collaboration_bridge,
                true,
                supervisor.run_stops.clone(),
            );
            supervisor
                .message_projects
                .lock()
                .unwrap()
                .remove(&project_id);
        });
        Ok(())
    }

    pub fn pause(
        &self,
        store: &MultiAgentStore,
        project_id: &str,
    ) -> Result<ProjectExecutionReceipt> {
        {
            let projects = self.projects.lock().unwrap();
            let messages = self.message_projects.lock().unwrap();
            store.set_project_status(project_id, ProjectStatus::Paused)?;
            if let Some(flag) = messages.get(project_id) { flag.store(true, Ordering::Release); }
            if let Some(flag) = projects.get(project_id) { flag.store(true, Ordering::Release); }
            for (owner,flag) in self.run_stops.lock().unwrap().values() {
                if owner==project_id { flag.store(true,Ordering::Release); }
            }
        }
        let started = Instant::now();
        while started.elapsed() < SUPERVISOR_STOP_TIMEOUT
            && (self.projects.lock().unwrap().contains_key(project_id)
                || self
                    .message_projects
                    .lock()
                    .unwrap()
                    .contains_key(project_id))
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        let stopped = !self.projects.lock().unwrap().contains_key(project_id)
            && !self
                .message_projects
                .lock()
                .unwrap()
                .contains_key(project_id);
        let unverified = store.snapshot(project_id)?.runs.iter().any(|run| run.session_locator.as_ref().is_some_and(|locator| locator["stopUnverified"] == true));
        Ok(ProjectExecutionReceipt {
            project_id: project_id.to_string(),
            accepted: stopped && !unverified,
            message: if unverified {
                "项目已暂停，但存在停止未确认的 Agent；请确认原进程已停止后解除冻结".to_string()
            } else if stopped {
                "项目已暂停；正在运行的 Agent 已停止，任务保留为可继续状态".to_string()
            } else {
                "项目已暂停，但 Agent 仍在退出中，请稍后再继续".to_string()
            },
        })
    }

    pub fn cancel(
        &self,
        store: &MultiAgentStore,
        project_id: &str,
    ) -> Result<ProjectExecutionReceipt> {
        // Wait for worker stop confirmation before terminalizing the Run. Otherwise
        // a failed kill could no longer be quarantined because cancellation ended it.
        let stopped = self.pause(store, project_id)?;
        if !stopped.accepted {
            return Ok(ProjectExecutionReceipt { project_id: project_id.to_string(), accepted: false, message: format!("取消尚未完成：{}", stopped.message) });
        }
        let count = store.cancel_project_tasks(project_id)?;
        Ok(ProjectExecutionReceipt {
            project_id: project_id.to_string(),
            accepted: true,
            message: format!("已取消项目和 {count} 个未完成任务"),
        })
    }

    /// Exact automation message Runs only. Never pauses a project or kills another member's work.
    pub fn stop_automation_runs(&self, store: &MultiAgentStore, targets: &[crate::automation::AutomationTarget]) -> Result<()> {
        let flags = self.run_stops.lock().unwrap();
        let mut selected = Vec::new();
        for target in targets {
            let project = target.project_id.as_ref().context("本轮不是受管理的项目任务，请打开原会话停止")?;
            let task_id = target.task_id.as_ref().context("缺少任务身份")?;
            if task_id != &format!("cli-message-{}", target.message_id) || !target.message_id.starts_with("automation-") { anyhow::bail!("自动化任务身份不匹配"); }
            let snapshot = store.snapshot(project)?;
            let run = snapshot.runs.iter().filter(|r| &r.task_id == task_id).max_by_key(|r| (r.attempt, r.created_at)).context("消息尚未启动，稍后重试撤销或打开原会话核对")?;
            if run.session_locator.as_ref().is_some_and(|l| l["stopUnverified"] == true) { anyhow::bail!("原进程停止未确认，请打开原会话核对"); }
            let (owner,flag)=flags.get(&run.id).context("本轮已结束或没有活动执行句柄，请刷新结果")?;
            if owner!=project {anyhow::bail!("执行句柄的项目身份已变化");}
            selected.push(flag.clone());
        }
        for flag in selected { flag.store(true, Ordering::Release); }
        Ok(())
    }
}

fn schedule_until_idle(
    store: &MultiAgentStore,
    project_id: &str,
    cancelled: Arc<AtomicBool>,
    terminal_host: AgentTerminalHost,
    collaboration_bridge: CollaborationBridge,
    message_only: bool,
    run_stops: Arc<Mutex<HashMap<String, (String, Arc<AtomicBool>)>>>,
) -> Result<()> {
    #[derive(Debug)]
    struct ActiveRun {
        agent_id: String,
        exclusive_write: bool,
        stop: Arc<AtomicBool>,
    }

    #[derive(Debug)]
    struct RunCompletion {
        run_id: String,
    }

    let (completion_tx, completion_rx) = mpsc::channel::<RunCompletion>();
    let mut active_runs = HashMap::<String, ActiveRun>::new();
    loop {
        let stopping = cancelled.load(Ordering::Acquire)
            || matches!(
                store.project(project_id)?.status,
                ProjectStatus::Paused | ProjectStatus::Cancelled
            );
        if stopping { for active in active_runs.values() { active.stop.store(true, Ordering::Release); } }
        if stopping && active_runs.is_empty() {
            return Ok(());
        }
        if stopping {
            if let Ok(completion) = completion_rx.recv_timeout(Duration::from_millis(100)) {
                active_runs.remove(&completion.run_id);
            }
            continue;
        }

        if !message_only {
            let _ = store.recover_expired_leases(project_id)?;
            let _ = store.route_runnable_tasks(project_id)?;
        }
        if !message_only {
            store.ensure_runnable_wakeups(project_id)?;
        }
        let candidates = store.runnable_tasks(project_id)?;
        let project = store.project(project_id)?;
        let git_project = is_git_worktree(PathBuf::from(&project.root_path).as_path());
        let mut scheduled_any = false;
        for (task, agent) in candidates {
            if message_only && !task.id.starts_with("cli-message-") {
                continue;
            }
            if cancelled.load(Ordering::Acquire) {
                break;
            }
            let needs_exclusive_write = task.workspace_mode == WorkspaceMode::SharedWrite
                || (task.workspace_mode == WorkspaceMode::Worktree && !git_project);
            if needs_exclusive_write && active_runs.values().any(|active| active.exclusive_write) {
                continue;
            }
            if active_runs
                .values()
                .any(|active| active.agent_id == agent.id)
            {
                continue;
            }
            if message_only {
                store.enqueue_wakeup(super::EnqueueWakeupInput {
                    project_id: project_id.to_string(),
                    agent_id: agent.id.clone(),
                    task_id: Some(task.id.clone()),
                    run_id: None,
                    reason: "处理会话消息".into(),
                    idempotency_key: format!("message:{}", task.id),
                })?;
            }
            let Some(wakeup) = store.claim_task_wakeup(&task.id)? else {
                continue;
            };
            let run = match store.claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id.clone(),
                lease_seconds: 60 * 60,
            }) {
                Ok(run) => run,
                Err(_) => {
                    let _ = store.finish_wakeup(&wakeup.id, false);
                    continue;
                }
            };
            let agent = store.agent_profile(&agent.id)?;
            scheduled_any = true;
            let run_cancelled = Arc::new(AtomicBool::new(cancelled.load(Ordering::Acquire)));
            run_stops.lock().unwrap().insert(run.id.clone(), (project_id.to_owned(),run_cancelled.clone()));
            // If pause won before registration, inherit it here; if it wins after,
            // pause sees this registered child flag and stops it directly.
            if cancelled.load(Ordering::Acquire) {run_cancelled.store(true,Ordering::Release);}
            active_runs.insert(
                run.id.clone(),
                ActiveRun {
                    agent_id: agent.id.clone(),
                    exclusive_write: needs_exclusive_write,
                    stop: run_cancelled.clone(),
                },
            );
            let store = store.clone();
            let project = project.clone();
            let run_stops = run_stops.clone();
            let terminal_host = terminal_host.clone();
            let collaboration_bridge = collaboration_bridge.clone();
            let completion_tx = completion_tx.clone();
            let run_id = run.id.clone();
            std::thread::spawn(move || {
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    execute_run(
                        &store,
                        project,
                        task,
                        agent,
                        run_id.clone(),
                        run_cancelled.as_ref(),
                        terminal_host,
                        collaboration_bridge,
                    )
                }));
                let succeeded = matches!(result, Ok(Ok(())));
                if result.is_err() {
                    let _ = store.fail_run_with_retry(
                        &run_id,
                        "执行线程异常退出，调度器已回收并按重试策略处理",
                        None,
                    );
                }
                let _ = store.finish_wakeup(&wakeup.id, succeeded);
                run_stops.lock().unwrap().remove(&run_id);
                let _ = completion_tx.send(RunCompletion { run_id });
            });
        }

        while let Ok(completion) = completion_rx.try_recv() {
            active_runs.remove(&completion.run_id);
        }

        if active_runs.is_empty() && !scheduled_any {
            if message_only {
                return Ok(());
            }
            let snapshot = store.snapshot(project_id)?;
            let now = now_millis_for_scheduler();
            let has_delayed_retry = snapshot.tasks.iter().any(|task| {
                matches!(
                    task.status,
                    super::TaskStatus::Ready | super::TaskStatus::ChangesRequested
                ) && task.next_retry_at.is_some_and(|retry_at| retry_at > now)
            });
            if has_delayed_retry {
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            match CollaborationGovernance::new(store.clone()).evaluate_quiescence(project_id)? {
                WatchdogDisposition::Scheduled => {
                    std::thread::sleep(Duration::from_millis(25));
                    continue;
                }
                WatchdogDisposition::WaitingForHuman => return Ok(()),
                WatchdogDisposition::ClearToComplete => {
                    store.set_project_status(project_id, ProjectStatus::Completed)?;
                    return Ok(());
                }
            }
        }

        if let Ok(completion) = completion_rx.recv_timeout(Duration::from_millis(100)) {
            active_runs.remove(&completion.run_id);
        }
    }
}

fn now_millis_for_scheduler() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod automation_stop_tests {
    use super::*;
    #[test]
    fn automation_stop_targets_one_run_and_refuses_ambiguous_batches() {
        let root=std::env::temp_dir().join(format!("automation-stop-{}",uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store=MultiAgentStore::open_at(root.join("store.db")).unwrap();
        let project=store.create_project(super::super::CreateProjectInput{name:"exact stop".into(),goal:"scope".into(),root_path:root.display().to_string(),default_branch:"main".into()}).unwrap();
        let agent=store.create_agent(serde_json::from_value(serde_json::json!({"projectId":project.id,"name":"member","runtimeKind":"codex","role":"worker"})).unwrap()).unwrap();
        store.enqueue_cli_message_task(&project.id,&agent.id,"automation-one","exact").unwrap();
        let run=store.claim_task(ClaimTaskInput{task_id:"cli-message-automation-one".into(),agent_id:agent.id,lease_seconds:60}).unwrap();
        let supervisor=MultiAgentSupervisor::default();let own=Arc::new(AtomicBool::new(false));let other=Arc::new(AtomicBool::new(false));
        supervisor.run_stops.lock().unwrap().insert(run.id.clone(),(project.id.clone(),own.clone()));
        supervisor.run_stops.lock().unwrap().insert("unrelated".into(),("another-project".into(),other.clone()));
        let target=crate::automation::AutomationTarget{session_id:"original".into(),message_id:"automation-one".into(),project_id:Some(project.id.clone()),task_id:Some(run.task_id.clone()),run_id:Some(run.id.clone()),stage:"running".into(),detail:String::new()};
        let mut wrong=target.clone();wrong.message_id="other".into();
        assert!(supervisor.stop_automation_runs(&store,&[target.clone(),wrong]).is_err());assert!(!own.load(Ordering::Acquire));
        supervisor.stop_automation_runs(&store,&[target.clone()]).unwrap();assert!(own.load(Ordering::Acquire));assert!(!other.load(Ordering::Acquire));
        assert_eq!(store.project(&project.id).unwrap().status,ProjectStatus::Active);
        assert!(store.cancel_queued_automation_tasks(&[target]).is_err(),"claimed task cannot be cancelled without stopping its Run");
        drop(store);std::fs::remove_dir_all(root).unwrap();
    }
}

struct RunHeartbeatGuard {
    stop: mpsc::Sender<()>,
    handle: Option<JoinHandle<()>>,
}

impl RunHeartbeatGuard {
    fn start(store: MultiAgentStore, run_id: String) -> Self {
        let (stop, receiver) = mpsc::channel();
        let handle = std::thread::spawn(move || loop {
            match receiver.recv_timeout(Duration::from_secs(15)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if store
                        .heartbeat_run(HeartbeatRunInput {
                            run_id: run_id.clone(),
                            lease_seconds: 120,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for RunHeartbeatGuard {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn validate_readonly_runtime(mode: WorkspaceMode, runtime: AgentRuntimeKind) -> Result<()> {
    if mode == WorkspaceMode::ReadOnly && !matches!(runtime, AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex) {
        anyhow::bail!("{} 暂不支持强制只读模式，请选用支持只读权限的 Agent 或明确改用其他工作区模式", runtime.as_str());
    }
    Ok(())
}

fn execution_workspace_mode(task: &MultiAgentTask, runtime: AgentRuntimeKind) -> WorkspaceMode {
    if super::conversation::is_conversation_task(&task.id) { return WorkspaceMode::ReadOnly; }
    // A mailbox turn runs in an empty, application-owned message directory; it
    // does not review the business workspace. Preserve enforced read-only mode
    // where available, but do not require unsupported review permissions just
    // to answer a message. The member's permission profile remains unchanged.
    if task.id.starts_with("cli-message-")
        && task.workspace_mode == WorkspaceMode::ReadOnly
        && matches!(
            runtime,
            AgentRuntimeKind::KimiCode
                | AgentRuntimeKind::Pi
                | AgentRuntimeKind::OpenCode
                | AgentRuntimeKind::DeepSeekHarness
                | AgentRuntimeKind::ZCode
                | AgentRuntimeKind::ZCodeCli
        )
    {
        WorkspaceMode::SharedWrite
    } else {
        task.workspace_mode
    }
}

fn message_workspace_at(config: &std::path::Path, task_id: &str) -> Result<PathBuf> {
    if !task_id.starts_with("cli-message-")
        || task_id.len() == "cli-message-".len()
        || task_id.contains(['/', '\\', ':', '\0'])
        || !matches!(
            std::path::Path::new(task_id).components().next(),
            Some(std::path::Component::Normal(_))
        )
    {
        anyhow::bail!("消息任务 ID 不是有效的单个路径分量，拒绝创建消息工作区");
    }
    Ok(config.join("message-workspaces").join(task_id))
}

fn is_isolated_message_workspace(
    config: &std::path::Path,
    task_id: &str,
    cwd: &std::path::Path,
    project_root: &std::path::Path,
) -> bool {
    let Ok(expected) = message_workspace_at(config, task_id) else { return false; };
    if cwd != expected { return false; }
    let (Ok(config), Ok(cwd), Ok(project)) = (
        resolved_scope_path(config), resolved_scope_path(cwd), resolved_scope_path(project_root),
    ) else { return false; };
    // A junction in the message directory must not make a business directory
    // eligible for Auto permissions. Neither directory may contain the other.
    let expected = config.join("message-workspaces").join(task_id);
    #[cfg(windows)]
    let expected = PathBuf::from(expected.to_string_lossy().to_lowercase());
    cwd == expected
        && !cwd.starts_with(&project) && !project.starts_with(&cwd)
}

pub(super) fn execute_run(
    store: &MultiAgentStore,
    project: MultiAgentProject,
    task: MultiAgentTask,
    agent: AgentProfile,
    run_id: String,
    cancelled: &AtomicBool,
    terminal_host: AgentTerminalHost,
    collaboration_bridge: CollaborationBridge,
) -> Result<()> {
    let _runtime_lease = match crate::runtime_preferences::acquire(agent.runtime_kind) {
        Ok(lease) => lease,
        Err(error) => {
            store.fail_run_permanently(&run_id, &error.to_string(), None)?;
            return Err(error);
        }
    };
    let mut agent = agent;
    agent.model = None;
    let mut task = task;
    task.workspace_mode = execution_workspace_mode(&task, agent.runtime_kind);
    let discussion = super::conversation::is_conversation_task(&task.id);
    if let Err(error) = if discussion && agent.runtime_kind == AgentRuntimeKind::Pi { Ok(()) } else { validate_readonly_runtime(task.workspace_mode, agent.runtime_kind) } {
        store.fail_run_permanently(&run_id, &error.to_string(), None)?;
        return Err(error);
    }
    let _heartbeat = RunHeartbeatGuard::start(store.clone(), run_id.clone());
    let _ = store.append_runtime_event(super::AppendRuntimeEventInput {
        run_id: run_id.clone(),
        event_type: "starting".to_string(),
        payload: serde_json::json!({"taskTitle": task.title, "runtime": agent.runtime_kind}),
    });
    let _ = store.append_run_log(&run_id, "system", &format!("准备执行任务：{}", task.title));
    let message_turn = task.id.starts_with("cli-message-");
    let resume_locator = if message_turn {
        // 消息是独立对话，不能把业务会话恢复到新的隔离目录。
        None
    } else {
        store.latest_session_locator(&task.id)?
    };
    let dependency_artifacts = store.dependency_artifacts(&task.id)?;
    let (resumable_cwd, resume_locator) = if message_turn {
        (None, resume_locator)
    } else {
        validated_resume_locator(&project, &task, resume_locator)
    };
    let workspace = if message_turn {
        (|| -> Result<PathBuf> {
            let cwd = message_workspace_at(&crate::providers::config_dir()?, &task.id)?;
            std::fs::create_dir_all(&cwd)?;
            Ok(cwd)
        })()
    } else {
        match resumable_cwd {
            Some(path) => Ok(path),
            None => prepare_workspace(&project, &task, &run_id, &dependency_artifacts),
        }
    };
    let cwd = match workspace {
        Ok(cwd) => cwd,
        Err(error) => {
            let _ = store.fail_run_with_retry(&run_id, &error.to_string(), None);
            return Err(error);
        }
    };
    let read_only_baseline_head =
        if task.workspace_mode == WorkspaceMode::ReadOnly && is_git_worktree(&cwd) {
            Some(git_head(&cwd)?)
        } else {
            None
        };
    let conversation_scope_baseline = if !message_turn && is_git_worktree(&cwd) { Some(git_head(&cwd)?) } else { None };
    let _ = store.append_run_log(&run_id, "system", &format!("工作目录：{}", cwd.display()));
    if message_turn && task.workspace_mode == WorkspaceMode::SharedWrite {
        let _ = store.append_run_log(&run_id, "system", "使用独立可写消息工作区并保留成员原权限；此 Runtime 不提供强制只读边界，禁止修改业务文件");
    }
    let bridge_lease =
        collaboration_bridge.register_run(&project.id, &task.id, &run_id, &agent.id, &cwd);
    let collaboration = bridge_lease.access();
    let _ = store.append_runtime_event(super::AppendRuntimeEventInput {
        run_id: run_id.clone(),
        event_type: "policy".to_string(),
        payload: serde_json::json!({
            "workspaceMode": task.workspace_mode,
            "permissionProfile": agent.permission_profile,
            "cwd": cwd,
            "agentCapabilities": agent.capabilities,
            "requiredCapabilities": task.required_capabilities,
            "maxAttempts": task.max_attempts,
            "collaborationScope": {
                "projectId": project.id,
                "taskId": task.id,
                "runId": run_id,
                "agentId": agent.id,
            }
        }),
    });
    store.update_run(UpdateRunInput {
        run_id: run_id.clone(),
        status: RunStatus::Running,
        output_summary: None,
        error: None,
        session_locator: Some(serde_json::json!({"cwd": cwd, "adapter": agent.runtime_kind})),
    })?;
    let user_instructions = if discussion { Vec::new() } else { store.claim_agent_instructions(&project.id, &agent.id, &task.id)? };
    let shared_memories = if discussion { Vec::new() } else { store.search_memory_for_agent(SearchMemoryInput {
        project_id: project.id.clone(),
        query: String::new(),
        namespace: None,
        current_run_id: Some(run_id.clone()),
        limit: 12,
    }, &agent.id)? };
    let collaboration_messages = if discussion { Vec::new() } else { store.scoped_messages(&project.id, &task.id, &agent.id, 50)? };
    let request = RuntimeRequest {
        run_id: run_id.clone(),
        cwd: cwd.clone(),
        project,
        task,
        agent: agent.clone(),
        dependency_artifacts,
        resume_locator,
        user_instructions,
        shared_memories,
        collaboration_messages,
        collaboration,
        store: store.clone(),
        terminal_host,
    };
    let adapter = adapter_for(agent.runtime_kind);
    debug_assert_eq!(adapter.kind(), agent.runtime_kind);
    match adapter.execute(&request, cancelled) {
        Ok(result) => {
            if discussion {
                store.append_runtime_event(super::AppendRuntimeEventInput {
                    run_id: run_id.clone(), event_type: "output".into(),
                    payload: serde_json::json!({"stream":"assistant","content":result.summary}),
                })?;
            }
            let _ = store.append_run_log(&run_id, "system", "Agent 执行结束，开始验收与保存交付物");
            let acceptance_verified = match verify_task_acceptance(
                &request.task,
                &cwd,
                cancelled,
                Some((&request.store, &request.run_id)),
            ) {
                Ok(verified) => verified,
                Err(error) => {
                    let _ = store.fail_run_with_retry(
                        &run_id,
                        &format!("验收命令失败：{error}"),
                        Some(result.session_locator),
                    );
                    return Err(error);
                }
            };
            let content_hash = if message_turn {
                None
            } else {
                if let Err(error) = verify_conversation_workspace_scope(store, &request.task.id, &cwd, conversation_scope_baseline.as_deref()) {
                    store.fail_run_permanently(&run_id, &error.to_string(), Some(result.session_locator))?;
                    return Err(error);
                }
                match checkpoint_task_workspace(&request, &cwd, read_only_baseline_head.as_deref())
                {
                    Ok(hash) => hash,
                    Err(error) => {
                        let _ = store.fail_run_with_retry(
                            &run_id,
                            &format!("保存任务工作区失败：{error}"),
                            Some(result.session_locator),
                        );
                        return Err(error);
                    }
                }
            };
            if let Err(error) = store.finalize_successful_run(
                &run_id,
                &cwd.display().to_string(),
                &result.summary,
                acceptance_verified,
                content_hash.as_deref(),
                Some(&result.session_locator),
            ) {
                let _ = store.fail_run_with_retry(
                    &run_id,
                    &format!("完成任务状态结算失败：{error}"),
                    Some(result.session_locator),
                );
                return Err(error);
            }
            let _ = store.append_run_log(&run_id, "system", "任务已完成并生成持久化交付记录");
            Ok(())
        }
        Err(error) => {
            if error.downcast_ref::<UnverifiedStop>().is_some() {
                store.mark_run_stop_unverified(&run_id, &format!("停止未获确认，禁止自动重试：{error}"))?;
                let _ = store.append_run_log(
                    &run_id,
                    "system",
                    &format!("停止未获确认，保留活动 Run 防止交接双执行：{error}"),
                );
                return Err(error);
            }
            if cancelled.load(Ordering::Acquire)
                && store.project(&request.project.id)?.status == ProjectStatus::Paused
            {
                store.pause_run(&run_id)?;
                let _ = store.append_run_log(
                    &run_id,
                    "system",
                    "项目已暂停，当前任务已停止并保留为可继续状态",
                );
                return Err(error);
            }
            let status = if cancelled.load(Ordering::Acquire) {
                RunStatus::Cancelled
            } else {
                RunStatus::Failed
            };
            if status == RunStatus::Failed {
                let locator = Some(serde_json::json!({"cwd": cwd}));
                if error.downcast_ref::<PermanentRunError>().is_some() {
                    let _ = store.fail_run_permanently(&run_id, &error.to_string(), locator);
                } else {
                    let _ = store.fail_run_with_retry(&run_id, &error.to_string(), locator);
                }
            } else {
                let _ = store.update_run(UpdateRunInput {
                    run_id: run_id.clone(),
                    status,
                    output_summary: None,
                    error: Some(error.to_string()),
                    session_locator: Some(serde_json::json!({"cwd": cwd})),
                });
            }
            let _ = store.append_run_log(&run_id, "stderr", &format!("执行失败：{error}"));
            Err(error)
        }
    }
}

fn validated_resume_locator(
    project: &MultiAgentProject,
    task: &MultiAgentTask,
    locator: Option<serde_json::Value>,
) -> (Option<PathBuf>, Option<serde_json::Value>) {
    let cwd = locator
        .as_ref()
        .and_then(|locator| locator.get("cwd"))
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .filter(|path| workspace_can_resume(project, task, path));
    if cwd.is_some() {
        (cwd, locator)
    } else {
        (None, None)
    }
}

fn workspace_can_resume(
    project: &MultiAgentProject,
    task: &MultiAgentTask,
    path: &std::path::Path,
) -> bool {
    if !path.is_dir() {
        return false;
    }
    let root = PathBuf::from(&project.root_path);
    match task.workspace_mode {
        WorkspaceMode::SharedWrite => path == root,
        WorkspaceMode::Worktree => is_git_worktree(&root) && path != root && is_git_worktree(path),
        WorkspaceMode::ReadOnly => false,
    }
}

pub(super) fn verify_conversation_workspace_scope(store: &MultiAgentStore, task_id: &str, cwd: &std::path::Path, baseline: Option<&str>) -> Result<()> {
    if !task_id.starts_with("conversation-task-") { return Ok(()); }
    let Some(baseline) = baseline else { return Err(anyhow!("执行计划需要可核验的 Git 工作区，不允许降级为共享目录")); };
    let mut paths = Vec::new();
    for arguments in [vec!["diff", "--relative", "--no-renames", "--name-only", "-z", baseline, "--"], vec!["ls-files", "--others", "--exclude-standard", "-z"]] {
        let output = hidden_command("git").arg("-C").arg(cwd).args(arguments).output().context("读取任务目录范围失败")?;
        if !output.status.success() { return Err(anyhow!("读取任务实际变更失败：{}", String::from_utf8_lossy(&output.stderr))); }
        paths.extend(output.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()).map(|p| String::from_utf8_lossy(p).into_owned()));
    }
    store.verify_conversation_write_scope(task_id, &paths)
}

fn checkpoint_task_workspace(
    request: &RuntimeRequest,
    cwd: &std::path::Path,
    read_only_baseline_head: Option<&str>,
) -> Result<Option<String>> {
    ensure_read_only_workspace_clean(&request.task, cwd, read_only_baseline_head)?;
    let project_root = PathBuf::from(&request.project.root_path);
    if request.task.workspace_mode != WorkspaceMode::Worktree
        || cwd == project_root
        || !is_git_worktree(cwd)
    {
        return Ok(workspace_content_hash(cwd));
    }
    let status = hidden_command("git")
        .arg("-C")
        .arg(cwd)
        .args(["status", "--porcelain"])
        .output()
        .context("读取任务 Worktree 状态失败")?;
    if !status.status.success() {
        return Err(anyhow!(
            "读取任务 Worktree 状态失败：{}",
            String::from_utf8_lossy(&status.stderr).trim()
        ));
    }
    if !status.stdout.is_empty() {
        let add = hidden_command("git")
            .arg("-C")
            .arg(cwd)
            .args(["add", "-A"])
            .output()
            .context("暂存任务 Worktree 变更失败")?;
        if !add.status.success() {
            return Err(anyhow!(
                "暂存任务 Worktree 变更失败：{}",
                String::from_utf8_lossy(&add.stderr).trim()
            ));
        }
        let commit = hidden_command("git")
            .arg("-C")
            .arg(cwd)
            .args([
                "-c",
                "user.name=CrewVela",
                "-c",
                "user.email=crewvela@local",
                "commit",
                "-m",
            ])
            .arg(format!("crewvela: {}", request.task.title))
            .output()
            .context("提交任务 Worktree 变更失败")?;
        if !commit.status.success() {
            return Err(anyhow!(
                "提交任务 Worktree 变更失败：{}",
                String::from_utf8_lossy(&commit.stderr).trim()
            ));
        }
    }
    let head = hidden_command("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("读取任务提交哈希失败")?;
    if !head.status.success() {
        return Err(anyhow!("读取任务提交哈希失败"));
    }
    Ok(Some(
        String::from_utf8_lossy(&head.stdout).trim().to_string(),
    ))
}

fn ensure_read_only_workspace_clean(
    task: &MultiAgentTask,
    cwd: &std::path::Path,
    baseline_head: Option<&str>,
) -> Result<()> {
    if task.workspace_mode != WorkspaceMode::ReadOnly || !is_git_worktree(cwd) {
        return Ok(());
    }
    let status = hidden_command("git")
        .arg("-C")
        .arg(cwd)
        .args(["status", "--porcelain"])
        .output()
        .context("检查 read_only 审查工作区状态失败")?;
    if !status.status.success() {
        return Err(anyhow!(
            "检查 read_only 审查工作区状态失败：{}",
            String::from_utf8_lossy(&status.stderr).trim()
        ));
    }
    if let Some(baseline_head) = baseline_head {
        let current_head = git_head(cwd)?;
        if current_head != baseline_head {
            return Err(anyhow!(
                "read_only 任务提交了审查工作区变更：基线 {baseline_head}，当前 {current_head}"
            ));
        }
    }
    if !status.stdout.is_empty() {
        return Err(anyhow!(
            "read_only 任务修改了审查工作区：{}",
            String::from_utf8_lossy(&status.stdout).trim()
        ));
    }
    Ok(())
}

pub fn merge_integrator_delivery(store: &MultiAgentStore, task_id: &str) -> Result<bool> {
    let Some((project_root, workspace, commit)) = store.integrator_delivery(task_id)? else {
        return Ok(false);
    };
    let root = PathBuf::from(project_root);
    let workspace = PathBuf::from(workspace);
    if root == workspace {
        return Err(anyhow!("集成 Worktree 与主工作区相同，无法执行审计合并"));
    }
    if !is_git_worktree(&root) || !is_git_worktree(&workspace) {
        return Err(anyhow!("集成交付或主工作区不是有效的 Git 工作区"));
    }
    if !valid_commit_hash(&commit) || !git_commit_exists(&workspace, &commit) {
        return Err(anyhow!("集成交付的提交哈希无效或已不存在，无法批准"));
    }
    let root_status = hidden_command("git")
        .arg("-C")
        .arg(&root)
        .args(["status", "--porcelain", "--untracked-files=no"])
        .output()
        .context("检查主工作区状态失败")?;
    if !root_status.status.success() || !root_status.stdout.is_empty() {
        return Err(anyhow!(
            "主工作区存在未提交变更，无法自动合并；请先处理后重试批准"
        ));
    }
    let merge = hidden_command("git")
        .arg("-C")
        .arg(&root)
        .args([
            "-c",
            "user.name=CrewVela",
            "-c",
            "user.email=crewvela@local",
            "merge",
            "--no-ff",
        ])
        .arg(&commit)
        .arg("-m")
        .arg(format!("crewvela: merge integrator {}", &commit[..12]))
        .output()
        .context("合并集成交付失败")?;
    if !merge.status.success() {
        let _ = hidden_command("git")
            .arg("-C")
            .arg(&root)
            .args(["merge", "--abort"])
            .status();
        return Err(anyhow!(
            "集成分支发生冲突，已中止合并：{}",
            String::from_utf8_lossy(&merge.stderr).trim()
        ));
    }
    Ok(true)
}

fn uses_posix_verification_shell(command: &str) -> bool {
    command
        .split("&&")
        .flat_map(|s| s.split("||"))
        .flat_map(|s| s.split(";"))
        .map(str::trim_start)
        .any(|s| {
            ["test ", "grep ", "sed ", "awk "]
                .iter()
                .any(|prefix| s.starts_with(prefix))
        })
}

#[cfg(target_os = "windows")]
fn git_bash_executable() -> Option<PathBuf> {
    git_bash_next_to(&which::which("git").ok()?, |candidate| candidate.is_file())
}

#[cfg(target_os = "windows")]
fn git_bash_next_to(
    git: &std::path::Path,
    exists: impl Fn(&std::path::Path) -> bool,
) -> Option<PathBuf> {
    // git.exe 常见落点：<root>\cmd|bin\git.exe（上两级为安装根）；Git Bash/MSYS
    // 环境里则是 <root>\mingw64\bin\git.exe（上三级才是安装根），两级都要尝试。
    let mut roots = Vec::with_capacity(2);
    if let Some(root) = git.parent().and_then(std::path::Path::parent) {
        roots.push(root);
        if let Some(parent) = root.parent() {
            roots.push(parent);
        }
    }
    roots
        .into_iter()
        .flat_map(|root| {
            [
                root.join("bin").join("bash.exe"),
                root.join("usr").join("bin").join("bash.exe"),
            ]
        })
        .find(|candidate| exists(candidate))
}

fn verification_process(verification: &str) -> Result<(Command, Option<String>)> {
    #[cfg(target_os = "windows")]
    {
        if uses_posix_verification_shell(verification) {
            let bash = git_bash_executable()
                .ok_or_else(|| anyhow!("验收命令使用 POSIX 工具，但未找到 Git Bash"))?;
            let mut command = hidden_command(bash);
            command.args(["-lc", verification]);
            return Ok((command, None));
        }
        let mut command = hidden_command("cmd.exe");
        command.args(["/d", "/q"]);
        return Ok((
            command,
            Some(format!("{verification}\r\nexit /b %errorlevel%\r\n")),
        ));
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut command = hidden_command("sh");
        command.args(["-lc", verification]);
        Ok((command, None))
    }
}
struct PipedCommandOutput {
    pid: u32,
    status: std::process::ExitStatus,
    stdout: String,
    stderr: String,
}

/// Drain both outputs before any potentially blocking input write. The owner
/// alone enforces cancellation and one deadline, including stdin and pipe EOF.
fn run_piped_command(
    mut command: Command,
    input: Option<String>,
    cancelled: &AtomicBool,
    timeout: Duration,
    operation: &str,
    readers: impl FnOnce(std::process::ChildStdout, std::process::ChildStderr) -> (RuntimeStreamReader, RuntimeStreamReader),
) -> Result<PipedCommandOutput> {
    let started = Instant::now();
    command.stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() }).stdout(Stdio::piped()).stderr(Stdio::piped());
    let (mut child, _identity) = crate::acp::spawn_owned_command(&mut command).with_context(|| format!("启动{operation}失败"))?;
    let pid = child.id();
    let stdout = child.stdout.take().context("子进程stdout不可用")?;
    let stderr = child.stderr.take().context("子进程stderr不可用")?;
    let (mut stdout, mut stderr) = readers(stdout, stderr);
    let (input_tx, input_rx) = mpsc::sync_channel(1);
    let mut writer = match input {
        Some(input) => {
            let mut stdin = child.stdin.take().context("子进程stdin不可用")?;
            Some(std::thread::spawn(move || {
                let result = stdin.write_all(input.as_bytes()).and_then(|_| stdin.flush());
                drop(stdin);
                let _ = input_tx.send(result);
            }))
        },
        None => None,
    };
    let result = (|| -> Result<std::process::ExitStatus> {
        let mut status = None;
        let mut exited_at = None;
        let mut input_done = writer.is_none();
        let mut stdout_done = false; let mut stderr_done = false;
        loop {
            if cancelled.load(Ordering::Acquire) { return Err(anyhow!("运行已取消")); }
            if started.elapsed() >= timeout { return Err(anyhow!("运行超时")); }
            if !input_done {
                match input_rx.try_recv() {
                    Ok(result) => { result.with_context(|| format!("写入{operation} Prompt失败"))?; input_done = true; },
                    Err(mpsc::TryRecvError::Empty) => {},
                    Err(mpsc::TryRecvError::Disconnected) => return Err(anyhow!("写入{operation} Prompt线程中断")),
                }
            }
            for (done, stream, label) in [(&mut stdout_done, &stdout, "stdout"), (&mut stderr_done, &stderr, "stderr")] {
                if !*done {
                    match stream.completion.try_recv() {
                        Ok(result) => { result.with_context(|| format!("读取{operation} {label}失败"))?; *done = true; },
                        Err(mpsc::TryRecvError::Empty) => {},
                        Err(mpsc::TryRecvError::Disconnected) => return Err(anyhow!("读取{operation} {label}线程中断")),
                    }
                }
            }
            if status.is_none() {
                status = crate::acp::try_wait_child_preserving_identity(&mut child).context("读取子进程状态失败")?;
                if status.is_some() { exited_at = Some(Instant::now()); }
            }
            if status.is_some() && input_done && stdout_done && stderr_done { return Ok(status.unwrap()); }
            if exited_at.is_some_and(|at| at.elapsed() >= STREAM_DRAIN_TIMEOUT) {
                return Err(anyhow!("{operation}已退出，但输入/输出管道未关闭，不能确认结果完整"));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    })();
    // Cleanup errors take precedence: a normal I/O failure must never release a
    // retry while the original command (or its descendants) may still execute.
    let stopped = stop_cli_checked(&mut child);
    stdout.active.store(false, Ordering::Release); stderr.active.store(false, Ordering::Release);
    cancel_synchronous_stream_read(&stdout); cancel_synchronous_stream_read(&stderr);
    if let Some(thread) = writer.as_ref() { cancel_stdin_write(thread); }
    if let Err(error) = stopped { return Err(error); }
    let cleanup_deadline = Instant::now() + STREAM_CANCEL_TIMEOUT;
    while Instant::now() < cleanup_deadline && (writer.as_ref().is_some_and(|thread| !thread.is_finished()) || stdout.thread.as_ref().is_some_and(|thread| !thread.is_finished()) || stderr.thread.as_ref().is_some_and(|thread| !thread.is_finished())) {
        std::thread::sleep(Duration::from_millis(5));
    }
    if writer.as_ref().is_some_and(|thread| thread.is_finished()) { if let Some(thread) = writer.take() { let _ = thread.join(); } }
    if stdout.thread.as_ref().is_some_and(|thread| thread.is_finished()) { join_runtime_stream_thread(&mut stdout)?; }
    if stderr.thread.as_ref().is_some_and(|thread| thread.is_finished()) { join_runtime_stream_thread(&mut stderr)?; }
    let status = result?;
    let stdout = stdout.capture.lock().map_err(|_| anyhow!("stdout缓存锁异常"))?.output();
    let stderr = stderr.capture.lock().map_err(|_| anyhow!("stderr缓存锁异常"))?.output();
    Ok(PipedCommandOutput { pid, status, stdout, stderr })
}

#[cfg(windows)]
fn cancel_stdin_write(thread: &JoinHandle<()>) {
    use std::os::windows::io::AsRawHandle;
    unsafe { let _ = windows_sys::Win32::System::IO::CancelSynchronousIo(thread.as_raw_handle()); }
}
#[cfg(not(windows))]
fn cancel_stdin_write(_thread: &JoinHandle<()>) {}

fn spawn_passive_stream_reader<R: Read + Send + 'static>(mut reader: R) -> RuntimeStreamReader {
    let capture = Arc::new(Mutex::new(RuntimeStreamCapture::default()));
    let reader_capture = Arc::clone(&capture); let active = Arc::new(AtomicBool::new(true)); let reader_active = Arc::clone(&active);
    let (sender, completion) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let result = (|| -> std::io::Result<()> {
            let mut bytes = [0u8; 8192];
            while reader_active.load(Ordering::Acquire) {
                let count = reader.read(&mut bytes)?;
                if count == 0 { break; }
                reader_capture.lock().map_err(|_| std::io::Error::other("输出缓存锁异常"))?.append(&bytes[..count]);
            }
            Ok(())
        })();
        let _ = sender.send(result);
    });
    RuntimeStreamReader { completion, capture, active, thread: Some(thread) }
}

fn verify_task_acceptance(
    task: &MultiAgentTask,
    cwd: &PathBuf,
    cancelled: &AtomicBool,
    telemetry: Option<(&MultiAgentStore, &str)>,
) -> Result<bool> {
    let Some(verification) = task
        .verification_command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return Ok(false);
    };
    if let Some((store, run_id)) = telemetry {
        let _ = store.append_run_log(run_id, "system", &format!("运行验收命令：{verification}"));
    }
    let (mut command, stdin_payload) = verification_process(verification)?;
    command
        .current_dir(external_command_path(cwd))
        .stdin(if stdin_payload.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = run_piped_command(command, stdin_payload, cancelled, Duration::from_secs(15 * 60), "验收命令", |stdout, stderr| (spawn_passive_stream_reader(stdout), spawn_passive_stream_reader(stderr)))?;
    let succeeded = output.status.success();
    let stdout = output.stdout; let stderr = output.stderr;
    if let Some((store, run_id)) = telemetry {
        if !stdout.trim().is_empty() {
            let _ = store.append_run_log(run_id, "verification", stdout.trim());
        }
        if !stderr.trim().is_empty() {
            let _ = store.append_run_log(run_id, "stderr", stderr.trim());
        }
    }
    if succeeded && verification_reports_zero_tests(verification, &stdout, &stderr) {
        return Err(anyhow!(
            "验收失败：命令成功退出，但没有执行任何测试\n{}",
            verification
        ));
    }
    if succeeded {
        Ok(true)
    } else {
        Err(anyhow!(
            "{}{}{}",
            verification,
            if stdout.trim().is_empty() { "" } else { "\n" },
            if stderr.trim().is_empty() {
                stdout.trim()
            } else {
                stderr.trim()
            }
        ))
    }
}

fn verification_reports_zero_tests(command: &str, stdout: &str, stderr: &str) -> bool {
    let normalized_command = command.trim().to_ascii_lowercase();
    let is_test_command = normalized_command.contains("npm test")
        || normalized_command.contains("npm run test")
        || normalized_command.contains("pnpm test")
        || normalized_command.contains("yarn test")
        || normalized_command.contains("node --test")
        || normalized_command.contains("cargo test")
        || normalized_command.contains("pytest")
        || normalized_command.contains("vitest")
        || normalized_command.contains("jest");
    if !is_test_command {
        return false;
    }
    let output =
        strip_terminal_control_sequences(&format!("{stdout}\n{stderr}")).to_ascii_lowercase();
    output.lines().map(str::trim).any(|line| {
        line == "tests 0"
            || line == "# tests 0"
            || line.ends_with(" tests 0")
            || line.contains("running 0 tests")
            || line.contains("collected 0 items")
            || line.contains("no tests found")
            || line.contains("no test files found")
    })
}

struct ClaudeAdapter;
struct CodexAdapter;
struct KimiCodeAdapter;
struct PiAdapter;
struct OpenCodeAdapter;
struct DeepSeekHarnessAdapter;
struct ZCodeAdapter(AgentRuntimeKind);

fn adapter_for(kind: AgentRuntimeKind) -> Box<dyn AgentRuntimeAdapter> {
    match kind {
        AgentRuntimeKind::ClaudeCode => Box::new(ClaudeAdapter),
        AgentRuntimeKind::Codex => Box::new(CodexAdapter),
        AgentRuntimeKind::KimiCode => Box::new(KimiCodeAdapter),
        AgentRuntimeKind::MiniMaxCode | AgentRuntimeKind::Unsupported => unreachable!("已移除的 CLI 不能执行"),
        AgentRuntimeKind::Pi => Box::new(PiAdapter),
        AgentRuntimeKind::OhMyPi => unreachable!("已移除的 Runtime 不能执行"),
        AgentRuntimeKind::OpenCode => Box::new(OpenCodeAdapter),
        AgentRuntimeKind::DeepSeekHarness => Box::new(DeepSeekHarnessAdapter),
        AgentRuntimeKind::ClaudeChat => unreachable!("claude_chat 仅用于独立原生聊天会话"),
        AgentRuntimeKind::KimiChat => unreachable!("kimi_chat 仅用于独立原生聊天会话"),
        AgentRuntimeKind::PiChat => unreachable!("pi_chat 仅用于独立原生聊天会话"),
        AgentRuntimeKind::CodexChat => unreachable!("codex_chat 仅用于独立原生聊天会话"),
        AgentRuntimeKind::OpenCodeChat => unreachable!("opencode_chat 仅用于独立原生聊天会话"),
        AgentRuntimeKind::ZCode => Box::new(ZCodeAdapter(AgentRuntimeKind::ZCode)),
        AgentRuntimeKind::ZCodeCli => Box::new(ZCodeAdapter(AgentRuntimeKind::ZCodeCli)),
    }
}

fn claude_permission_settings(
    workspace_mode: WorkspaceMode,
    permission_profile: PermissionProfile,
) -> (&'static str, bool) {
    if workspace_mode == WorkspaceMode::ReadOnly {
        return ("plan", false);
    }
    match permission_profile {
        PermissionProfile::Default => ("dontAsk", false),
        PermissionProfile::Auto => ("auto", false),
        PermissionProfile::Yolo => ("bypassPermissions", true),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CodexPermissionSettings {
    ReadOnly,
    DangerousBypass,
    Unsandboxed { approval: &'static str },
    Sandboxed { approval: &'static str },
}

fn codex_permission_settings(
    workspace_mode: WorkspaceMode,
    permission_profile: PermissionProfile,
) -> CodexPermissionSettings {
    if workspace_mode == WorkspaceMode::ReadOnly {
        return CodexPermissionSettings::ReadOnly;
    }
    match permission_profile {
        PermissionProfile::Yolo => CodexPermissionSettings::DangerousBypass,
        PermissionProfile::Auto => {
            #[cfg(target_os = "windows")]
            {
                CodexPermissionSettings::Unsandboxed { approval: "never" }
            }
            #[cfg(not(target_os = "windows"))]
            {
                CodexPermissionSettings::Sandboxed { approval: "never" }
            }
        }
        PermissionProfile::Default => CodexPermissionSettings::Sandboxed {
            approval: "on-request",
        },
    }
}

fn uses_dangerous_runtime_permissions(
    workspace_mode: WorkspaceMode,
    permission_profile: PermissionProfile,
) -> bool {
    workspace_mode != WorkspaceMode::ReadOnly
        && matches!(
            permission_profile,
            PermissionProfile::Auto | PermissionProfile::Yolo
        )
}

fn task_requires_session_cli(task: &MultiAgentTask) -> bool {
    contract_requires_session_cli(&task.description, &task.acceptance_criteria)
}

fn request_requires_session_cli(request: &RuntimeRequest) -> Result<bool> {
    if super::conversation::is_conversation_task(&request.task.id) { return Ok(false); }
    Ok(task_requires_session_cli(&request.task)
        || request
            .store
            .snapshot(&request.project.id)?
            .agents
            .iter()
            .filter(|agent| agent.enabled)
            .map(|agent| agent.runtime_kind.as_str())
            .collect::<HashSet<_>>()
            .len()
            > 1)
}

fn configure_codex_cli_sandbox(
    command: &mut Command,
    readonly_cwd: Option<&std::path::Path>,
    outbox: &std::path::Path,
) {
    if let Some(scratch) = readonly_cwd {
        // workspace-write always grants CWD; never use the business checkout here.
        command.arg("-C").arg(external_command_path(scratch));
        command.args(["-s", "workspace-write", "-a", "never"]);
        command
            .arg("-c")
            .arg("sandbox_workspace_write.writable_roots=[]");
        command
            .arg("-c")
            .arg("sandbox_workspace_write.exclude_tmpdir_env_var=true");
        command
            .arg("-c")
            .arg("sandbox_workspace_write.exclude_slash_tmp=true");
    }
    command.arg("--add-dir").arg(external_command_path(outbox));
}

fn resolved_scope_path(path: &std::path::Path) -> Result<PathBuf> {
    let mut ancestor = std::path::absolute(path)?;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(
            ancestor
                .file_name()
                .context("无法解析通信权限路径")?
                .to_os_string(),
        );
        if !ancestor.pop() {
            return Err(anyhow!("无法解析通信权限路径"));
        }
    }
    let mut resolved = ancestor.canonicalize()?;
    for component in suffix.into_iter().rev() {
        resolved.push(component);
    }
    #[cfg(windows)]
    {
        resolved = PathBuf::from(resolved.to_string_lossy().to_lowercase());
    }
    Ok(resolved)
}

fn validate_readonly_cli_writable_scope(
    cwd: &std::path::Path,
    project: &std::path::Path,
    scratch: &std::path::Path,
    outbox: &std::path::Path,
) -> Result<()> {
    for business in [resolved_scope_path(cwd)?, resolved_scope_path(project)?] {
        for writable in [resolved_scope_path(scratch)?, resolved_scope_path(outbox)?] {
            if writable.starts_with(&business) || business.starts_with(&writable) {
                return Err(anyhow!(
                    "只读项目与通信可写目录重叠，拒绝开放业务目录写权限；请使用独立项目目录"
                ));
            }
        }
    }
    Ok(())
}

fn configure_codex_session_cli_permissions(
    command: &mut Command,
    request: &RuntimeRequest,
) -> Result<()> {
    let outbox = crate::providers::config_dir()?.join("outbox");
    let scratch = if request.task.workspace_mode == WorkspaceMode::ReadOnly {
        let scratch = crate::providers::config_dir()?
            .join("multi-agent-readonly")
            .join(&request.project.id)
            .join(&request.agent.id);
        validate_readonly_cli_writable_scope(
            &request.cwd,
            std::path::Path::new(&request.project.root_path),
            &scratch,
            &outbox,
        )?;
        std::fs::create_dir_all(&scratch)?;
        Some(scratch)
    } else {
        None
    };
    std::fs::create_dir_all(&outbox)?;
    configure_codex_cli_sandbox(command, scratch.as_deref(), &outbox);
    Ok(())
}

fn managed_acp_mode(
    workspace_mode: WorkspaceMode,
    permission_profile: PermissionProfile,
) -> ManagedAcpMode {
    if workspace_mode == WorkspaceMode::ReadOnly {
        return ManagedAcpMode::ReadOnly;
    }
    match permission_profile {
        PermissionProfile::Default => ManagedAcpMode::Default,
        PermissionProfile::Auto => ManagedAcpMode::Auto,
        PermissionProfile::Yolo => ManagedAcpMode::FullAccess,
    }
}

fn execute_preferred_acp(
    kind: ManagedAcpKind,
    locator_keys: &[&str],
    request: &RuntimeRequest,
    cancelled: &AtomicBool,
) -> Result<Option<RuntimeResult>> {
    if kind != ManagedAcpKind::DeepSeekHarness && request_requires_session_cli(request)? {
        return Ok(None);
    }
    let resume_session_id = locator_string(request.resume_locator.as_ref(), locator_keys);
    let mode = managed_acp_mode(
        request.task.workspace_mode,
        request.agent.permission_profile,
    );
    let deepseek_policy = match mode {
        ManagedAcpMode::ReadOnly => "read-only",
        ManagedAcpMode::FullAccess => "danger-full-access",
        _ => "workspace-write",
    };
    if kind == ManagedAcpKind::DeepSeekHarness && resume_session_id.is_some() {
        let locator = request
            .resume_locator
            .as_ref()
            .context("缺少 DeepSeek Harness 恢复信息")?;
        validate_deepseek_resume(
            locator,
            &request.cwd,
            deepseek_policy,
            request.agent.permission_profile,
        )?;
    }
    let session = request
        .terminal_host
        .sessions
        .create_agent(NewAgentSession {
            avatar_id: request.agent.avatar_id.clone(),
            project_id: request.project.id.clone(),
            agent_id: request.agent.id.clone(),
            runtime_kind: request.agent.runtime_kind.as_str().to_string(),
            agent_name: request.agent.name.clone(),
            cwd: request.cwd.display().to_string(),
        })?;
    request
        .store
        .link_agent_app_session(&request.project.id, &request.agent.id, &session.id)?;
    request
        .terminal_host
        .sessions
        .set_runtime_command_host(&session.id, true)?;
    request.terminal_host.sessions.mark_agent_activity(
        &session.id,
        SessionStatus::Running,
        None,
    )?;
    let permission_handler: Option<crate::acp::ManagedPermissionHandler> =
        if kind == ManagedAcpKind::DeepSeekHarness && mode == ManagedAcpMode::Default {
            let store = request.store.clone();
            let project_id = request.project.id.clone();
            let task_id = request.task.id.clone();
            let run_id = request.run_id.clone();
            Some(Box::new(move |value, cancellation| {
                let title = value
                    .pointer("/params/toolCall/title")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("Agent 工具调用");
                let approval = store.request_approval(super::RequestApprovalInput {
                    project_id: project_id.clone(),
                    task_id: Some(task_id.clone()),
                    run_id: Some(run_id.clone()),
                    approval_type: "runtime_permission".into(),
                    summary: format!(
                        "DeepSeek Harness 请求执行：{title}\n{}",
                        runtime_permission_details(
                            value
                                .pointer("/params/toolCall/rawInput")
                                .unwrap_or(&serde_json::Value::Null)
                        )
                    ),
                })?;
                let start = Instant::now();
                let mut heartbeat = Instant::now();
                let outcome = loop {
                    if cancellation.is_some_and(|flag| flag.load(Ordering::Acquire)) {
                        break Err(anyhow!("ACP Agent 任务已取消"));
                    }
                    if start.elapsed() > Duration::from_secs(30 * 60) {
                        break Err(anyhow!("DeepSeek Harness 权限审批等待超时"));
                    }
                    let snapshot = match store.snapshot(&project_id) {
                        Ok(snapshot) => snapshot,
                        Err(error) => break Err(error),
                    };
                    match snapshot
                        .approvals
                        .iter()
                        .find(|item| item.id == approval.id)
                        .map(|item| item.status.as_str())
                    {
                        Some("approved") => break Ok(true),
                        Some("pending") => {}
                        _ => break Ok(false),
                    }
                    if heartbeat.elapsed() > Duration::from_secs(30) {
                        if let Err(error) = store.heartbeat_run(HeartbeatRunInput {
                            run_id: run_id.clone(),
                            lease_seconds: 120,
                        }) {
                            break Err(error);
                        }
                        heartbeat = Instant::now();
                    }
                    std::thread::sleep(Duration::from_millis(100));
                };
                if !matches!(outcome, Ok(true)) {
                    store.cancel_runtime_approval(&approval.id)?;
                }
                outcome
            }))
        } else {
            None
        };
    let discussion = super::conversation::is_conversation_task(&request.task.id);
    let mcp_url = if discussion { String::new() } else { format!("{}/mcp", request.collaboration.endpoint) };
    let event_store = request.store.clone();
    let event_run = request.run_id.clone();
    let update_handler: Option<Box<dyn FnMut(&serde_json::Value) + Send>> = if discussion { Some(Box::new(move |update| {
        if update.get("sessionUpdate").and_then(serde_json::Value::as_str) == Some("agent_message_chunk") {
            if let Some(text) = update.pointer("/content/text").and_then(serde_json::Value::as_str) {
                if let Err(error) = event_store.append_runtime_event(super::AppendRuntimeEventInput {run_id:event_run.clone(),event_type:"assistant_delta".into(),payload:serde_json::json!({"text":text})}) {
                    eprintln!("[crewvela] 公共讨论回复持久化失败：{error:#}");
                }
            }
        }
    })) } else { None };
    request.store.mark_acp_execution(&request.run_id)?;
    let outcome = execute_managed_acp(
        kind,
        &session.id,
        &request.cwd.display().to_string(),
        resume_session_id.as_deref(),
        &render_task_prompt(request),
        &mcp_url,
        &request.collaboration.token,
        managed_acp_mode(
            request.task.workspace_mode,
            request.agent.permission_profile,
        ),
        cancelled,
        None,
        permission_handler,
        update_handler,
    );
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            // 与 PTY 路径一致：仅项目暂停导致的取消才标记 Stopped，其余记 Error
            let status = if error.downcast_ref::<UnverifiedStop>().is_some() { SessionStatus::Error } else if cancelled.load(Ordering::Acquire)
                && request
                    .store
                    .project(&request.project.id)
                    .is_ok_and(|project| project.status == ProjectStatus::Paused)
            {
                SessionStatus::Stopped
            } else {
                SessionStatus::Error
            };
            let _ = request.terminal_host.sessions.mark_agent_activity(
                &session.id,
                status,
                Some(error.to_string()),
            );
            return Err(error);
        }
    };
    match outcome {
        ManagedAcpOutcome::Completed(result) => {
            request.terminal_host.sessions.mark_agent_activity(
                &session.id,
                SessionStatus::Completed,
                None,
            )?;
            let _ = request.store.append_run_log(
                &request.run_id,
                "system",
                &format!(
                    "{} 通过 ACP {} 执行",
                    request.agent.name, result.agent_version
                ),
            );
            Ok(Some(RuntimeResult {
                summary: result.text,
                session_locator: serde_json::json!({
                    "adapter": "acp",
                    "protocol": "acp-v1",
                    "cwd": request.cwd,
                    "sessionId": result.session_id,
                    "agentName": result.agent_name,
                    "agentVersion": result.agent_version,
                    "permissionMode": if kind == ManagedAcpKind::DeepSeekHarness { Some(deepseek_policy) } else { None },
                    "permissionProfile": request.agent.permission_profile,
                }),
            }))
        }
        ManagedAcpOutcome::Unavailable(reason) => {
            if discussion { return Err(anyhow!("只读讨论 ACP 未就绪，不降级为权限不同的执行路径：{reason}")); }
            if kind == ManagedAcpKind::DeepSeekHarness {
                request.terminal_host.sessions.mark_agent_activity(
                    &session.id,
                    SessionStatus::Error,
                    Some(reason.clone()),
                )?;
                return Err(anyhow!("DeepSeek Harness ACP 启动失败：{reason}"));
            }
            request.store.clear_acp_execution(&request.run_id)?;
            request.terminal_host.sessions.mark_agent_activity(
                &session.id,
                SessionStatus::Pending,
                None,
            )?;
            let _ = request.store.append_run_log(
                &request.run_id,
                "system",
                &format!("ACP 不可用，回退 PTY：{reason}"),
            );
            Ok(None)
        }
    }
}

impl AgentRuntimeAdapter for DeepSeekHarnessAdapter {
    fn kind(&self) -> AgentRuntimeKind {
        AgentRuntimeKind::DeepSeekHarness
    }
    fn execute(&self, request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult> {
        execute_preferred_acp(
            ManagedAcpKind::DeepSeekHarness,
            &["sessionId"],
            request,
            cancelled,
        )?
        .context("DeepSeek Harness 必须使用 ACP 执行，不能降级为 PTY")
    }
}

impl AgentRuntimeAdapter for ZCodeAdapter {
    fn kind(&self) -> AgentRuntimeKind {
        self.0
    }
    fn execute(&self, request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult> {
        let label = if self.0 == AgentRuntimeKind::ZCodeCli {
            "ZCode CLI"
        } else {
            "ZCode Desktop"
        };
        if request.task.workspace_mode == WorkspaceMode::ReadOnly {
            return Err(PermanentRunError(
                format!("{label} 尚无经验证的强制只读边界，拒绝执行只读任务；请选用支持只读的 Runtime"),
            )
            .into());
        }
        if request.agent.permission_profile == PermissionProfile::Auto {
            require_isolated_auto_approval(request, label)?;
        }
        super::zcode::model_selection(None)
            .map_err(|error| PermanentRunError(error.to_string()))?;
        super::zcode::runtime_executable(self.0)
            .map_err(|error| PermanentRunError(error.to_string()))?;
        let mode = if request.agent.permission_profile == PermissionProfile::Yolo {
            "yolo"
        } else {
            "build"
        };
        let resume = zcode_resume_session(
            request.resume_locator.as_ref(),
            &request.cwd,
            request.agent.permission_profile,
            None,
        )
        .map_err(|error| PermanentRunError(error.to_string()))?;
        let session = request
            .terminal_host
            .sessions
            .create_agent(NewAgentSession {
                avatar_id: request.agent.avatar_id.clone(),
                project_id: request.project.id.clone(),
                agent_id: request.agent.id.clone(),
                runtime_kind: self.0.as_str().into(),
                agent_name: request.agent.name.clone(),
                cwd: request.cwd.display().to_string(),
            })?;
        request.store.link_agent_app_session(
            &request.project.id,
            &request.agent.id,
            &session.id,
        )?;
        request
            .terminal_host
            .sessions
            .set_runtime_command_host(&session.id, true)?;
        request.terminal_host.sessions.mark_agent_activity(
            &session.id,
            SessionStatus::Running,
            None,
        )?;
        let mut permission =
            |params: &serde_json::Value, cancellation: &AtomicBool| -> Result<bool> {
                if params
                    .get("sessionId")
                    .and_then(serde_json::Value::as_str)
                    .is_none()
                {
                    return Err(anyhow!("{label} 审批缺少会话身份"));
                }
                if request.agent.permission_profile != PermissionProfile::Default {
                    return Ok(true);
                }
                let tool = params
                    .get("toolName")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("工具");
                let approval = request
                    .store
                    .request_approval(super::RequestApprovalInput {
                        project_id: request.project.id.clone(),
                        task_id: Some(request.task.id.clone()),
                        run_id: Some(request.run_id.clone()),
                        approval_type: "runtime_permission".into(),
                        summary: format!(
                            "{label} 请求执行：{tool}\n{}",
                            runtime_permission_details(&params["input"])
                        ),
                    })?;
                let start = Instant::now();
                let mut heartbeat = Instant::now();
                let outcome = loop {
                    if cancellation.load(Ordering::Acquire) {
                        break Err(anyhow!("{label} 任务已取消"));
                    }
                    if start.elapsed() > Duration::from_secs(30 * 60) {
                        break Err(anyhow!("{label} 权限审批等待超时"));
                    }
                    let snapshot = match request.store.snapshot(&request.project.id) {
                        Ok(value) => value,
                        Err(error) => break Err(error),
                    };
                    match snapshot
                        .approvals
                        .iter()
                        .find(|item| item.id == approval.id)
                        .map(|item| item.status.as_str())
                    {
                        Some("approved") => break Ok(true),
                        Some("pending") => {}
                        _ => break Ok(false),
                    }
                    if heartbeat.elapsed() > Duration::from_secs(20) {
                        if let Err(error) = request.store.heartbeat_run(HeartbeatRunInput {
                            run_id: request.run_id.clone(),
                            lease_seconds: 120,
                        }) {
                            break Err(error);
                        }
                        heartbeat = Instant::now();
                    }
                    std::thread::sleep(Duration::from_millis(100));
                };
                if !matches!(outcome, Ok(true)) {
                    request.store.cancel_runtime_approval(&approval.id)?;
                }
                outcome
            };
        let mut progress = || {
            request
                .store
                .heartbeat_run(HeartbeatRunInput {
                    run_id: request.run_id.clone(),
                    lease_seconds: 120,
                })
                .map(|_| ())
        };
        let outcome = super::zcode::execute(
            self.0,
            &request.cwd,
            resume.as_deref(),
            &render_task_prompt(request),
            mode,
            None,
            serde_json::json!([{"name":"crewvela","type":"http","url":format!("{}/mcp",request.collaboration.endpoint),"headers":[{"name":"Authorization","value":format!("Bearer {}",request.collaboration.token)}],"isolation":"session"}]),
            cancelled,
            &mut permission,
            &mut progress,
        );
        match outcome {
            Ok(result) => {
                request.terminal_host.sessions.mark_agent_activity(
                    &session.id,
                    SessionStatus::Completed,
                    None,
                )?;
                request.store.append_run_log(
                    &request.run_id,
                    "system",
                    &format!("{label} 原生 v4 Agent 已确认当前任务终态"),
                )?;
                Ok(RuntimeResult {
                    summary: result.text,
                    session_locator: serde_json::json!({"adapter":"zcode_native","protocol":"zcode-v4","cwd":request.cwd,"sessionId":result.session_id,"permissionProfile":request.agent.permission_profile,"model":request.agent.model}),
                })
            }
            Err(error) => {
                let _ = request.terminal_host.sessions.mark_agent_activity(
                    &session.id,
                    SessionStatus::Error,
                    Some(error.to_string()),
                );
                Err(error)
            }
        }
    }
}

fn zcode_resume_session(
    locator: Option<&serde_json::Value>,
    cwd: &std::path::Path,
    profile: PermissionProfile,
    model: Option<&str>,
) -> Result<Option<String>> {
    let Some(locator) = locator else {
        return Ok(None);
    };
    // 公共失败检查点只有 cwd；公共启动检查点另有 adapter:"zcode"，都没有 Agent 会话可恢复。
    if locator.as_object().is_some_and(|object| {
        (object.len() == 1
            || (object.len() == 2
                && object.get("adapter").and_then(serde_json::Value::as_str) == Some("zcode")))
            && object.get("cwd").and_then(serde_json::Value::as_str)
                == Some(cwd.to_string_lossy().as_ref())
    }) {
        return Ok(None);
    }
    validate_zcode_resume(locator, cwd, profile, model)?;
    Ok(locator
        .get("sessionId")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned))
}

fn validate_zcode_resume(
    locator: &serde_json::Value,
    cwd: &std::path::Path,
    profile: PermissionProfile,
    model: Option<&str>,
) -> Result<()> {
    if locator.get("adapter").and_then(serde_json::Value::as_str) != Some("zcode_native")
        || locator.get("protocol").and_then(serde_json::Value::as_str) != Some("zcode-v4")
        || !locator
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .is_some_and(|id| !id.trim().is_empty())
        || !locator
            .get("model")
            .is_some_and(|value| value.is_null() || value.is_string())
        || locator.get("cwd").and_then(serde_json::Value::as_str)
            != Some(cwd.to_string_lossy().as_ref())
        || locator
            .get("permissionProfile")
            .and_then(serde_json::Value::as_str)
            != Some(profile.as_str())
        || locator.get("model").and_then(serde_json::Value::as_str) != model
    {
        return Err(anyhow!(
            "ZCode Desktop 恢复信息无效，或目录、权限、模型已变化，请新建会话"
        ));
    }
    Ok(())
}

fn validate_deepseek_resume(
    locator: &serde_json::Value,
    cwd: &std::path::Path,
    mode: &str,
    profile: PermissionProfile,
) -> Result<()> {
    if locator
        .get("permissionMode")
        .and_then(serde_json::Value::as_str)
        != Some(mode)
        || locator
            .get("permissionProfile")
            .and_then(serde_json::Value::as_str)
            != Some(profile.as_str())
        || locator.get("cwd").and_then(serde_json::Value::as_str)
            != Some(cwd.to_string_lossy().as_ref())
    {
        return Err(anyhow!(
            "DeepSeek Harness 已有会话的工作目录或权限已变化，请重新创建会话；未恢复旧权限"
        ));
    }
    Ok(())
}

fn runtime_permission_details(value: &serde_json::Value) -> String {
    fn redact(value: &serde_json::Value) -> serde_json::Value {
        match value {
            serde_json::Value::Object(object) => serde_json::Value::Object(
                object
                    .iter()
                    .map(|(key, value)| {
                        let lower = key.to_ascii_lowercase();
                        let private = [
                            "token",
                            "secret",
                            "password",
                            "authorization",
                            "api_key",
                            "apikey",
                            "content",
                            "env",
                        ]
                        .iter()
                        .any(|word| lower.contains(word));
                        (
                            key.clone(),
                            if private {
                                serde_json::json!("[已隐藏]")
                            } else {
                                redact(value)
                            },
                        )
                    })
                    .collect(),
            ),
            serde_json::Value::Array(values) => {
                serde_json::Value::Array(values.iter().map(redact).collect())
            }
            serde_json::Value::String(text) => {
                let lower = text.to_ascii_lowercase();
                if [
                    "bearer ",
                    "api_key",
                    "api-key",
                    "apikey",
                    "token=",
                    "token ",
                    "password=",
                    "secret=",
                    "sk-",
                ]
                .iter()
                .any(|word| lower.contains(word))
                {
                    serde_json::json!("[含敏感字段，已隐藏]")
                } else {
                    value.clone()
                }
            }
            _ => value.clone(),
        }
    }
    if value.is_null() {
        return "未提供工具参数".into();
    }
    let rendered = serde_json::to_string(&redact(value)).unwrap_or_default();
    rendered.chars().take(4000).collect()
}
/// claude 原生路径的 --mcp-config：crewvela 协作桥 + 保留配置中启用的用户 server。
/// 同名时 crewvela 优先（控制面不可被用户配置顶掉）。
fn claude_agent_mcp_config(
    user_servers: &serde_json::Map<String, serde_json::Value>,
    mcp_url: &str,
    token: &str,
) -> serde_json::Value {
    let mut servers = user_servers.clone();
    servers.insert(
        "crewvela".into(),
        serde_json::json!({
            "type": "http",
            "url": mcp_url,
            "headers": {
                "Authorization": format!("Bearer {token}")
            }
        }),
    );
    serde_json::json!({ "mcpServers": servers })
}

fn configure_collaboration_mcp(command: &mut Command, request: &RuntimeRequest) -> Result<()> {
    if super::conversation::is_conversation_task(&request.task.id) { return Ok(()); }
    let mcp_url = format!("{}/mcp", request.collaboration.endpoint);
    match request.agent.runtime_kind {
        AgentRuntimeKind::ClaudeCode => {
            let user_servers = crate::mcp::enabled_claude_map().unwrap_or_else(|error| {
                eprintln!("[crewvela] 读取用户 MCP 配置失败：{error:#}");
                serde_json::Map::new()
            });
            let config =
                claude_agent_mcp_config(&user_servers, &mcp_url, &request.collaboration.token);
            command.arg("--mcp-config").arg(config.to_string());
        }
        AgentRuntimeKind::Codex => {
            command.env("CREWVELA_AGENT_TOKEN", &request.collaboration.token);
            command.arg("-c").arg(format!(
                "mcp_servers.crewvela.url={}",
                serde_json::to_string(&mcp_url)?
            ));
            command
                .arg("-c")
                .arg("mcp_servers.crewvela.bearer_token_env_var=\"CREWVELA_AGENT_TOKEN\"");
        }
        AgentRuntimeKind::KimiCode => {
            command.env("CREWVELA_AGENT_TOKEN", &request.collaboration.token);
        }
        AgentRuntimeKind::Unsupported => return Err(anyhow!("该 Runtime 已移除或不受支持，请更换 Runtime 后重试")),
        AgentRuntimeKind::MiniMaxCode => return Err(anyhow!("MiniMax CLI 已移除，请更换 Runtime 后重试")),
        AgentRuntimeKind::Pi => {
            command.env("CREWVELA_PI_MCP_URL", mcp_url);
            command.env("CREWVELA_AGENT_TOKEN", &request.collaboration.token);
        }
        AgentRuntimeKind::OpenCode
        => {}
        AgentRuntimeKind::DeepSeekHarness
        | AgentRuntimeKind::ClaudeChat
        | AgentRuntimeKind::KimiChat
        | AgentRuntimeKind::PiChat
        | AgentRuntimeKind::CodexChat
        | AgentRuntimeKind::OpenCodeChat
        | AgentRuntimeKind::ZCode
        | AgentRuntimeKind::ZCodeCli => {}
        AgentRuntimeKind::OhMyPi => return Err(anyhow!("Oh My Pi 已移除，请更换 Runtime 后重试")),
    }
    Ok(())
}

fn configure_codex_base(command: &mut Command, request: &RuntimeRequest) -> Result<()> {
    let session = request
        .terminal_host
        .sessions
        .create_agent(NewAgentSession {
            avatar_id: request.agent.avatar_id.clone(),
            project_id: request.project.id.clone(),
            agent_id: request.agent.id.clone(),
            runtime_kind: request.agent.runtime_kind.as_str().to_string(),
            agent_name: request.agent.name.clone(),
            cwd: request.cwd.display().to_string(),
        })?;
    crate::pty::configure_codex_cli_identity(command, &session.id)?;
    configure_collaboration_mcp(command, request)?;
    let needs_cli = request_requires_session_cli(request)?;
    if !(needs_cli && request.task.workspace_mode == WorkspaceMode::ReadOnly) {
        command.arg("-C").arg(external_command_path(&request.cwd));
    }
    let settings = if super::conversation::is_conversation_task(&request.task.id) {
        CodexPermissionSettings::ReadOnly
    } else if request.task.id.starts_with("cli-message-") {
        CodexPermissionSettings::Sandboxed {
            approval: "on-request",
        }
    } else {
        codex_permission_settings(
            request.task.workspace_mode,
            request.agent.permission_profile,
        )
    };
    if !(needs_cli && request.task.workspace_mode == WorkspaceMode::ReadOnly) {
        match settings {
            CodexPermissionSettings::ReadOnly => {
                command.args(["-s", "read-only", "-a", "never"]);
            }
            CodexPermissionSettings::DangerousBypass => {
                command.arg("--dangerously-bypass-approvals-and-sandbox");
            }
            CodexPermissionSettings::Unsandboxed { approval } => {
                command.args(["-s", "danger-full-access", "-a", approval]);
            }
            CodexPermissionSettings::Sandboxed { approval } => {
                command.args(["-s", "workspace-write", "-a", approval]);
            }
        }
    }
    if needs_cli {
        configure_codex_session_cli_permissions(command, request)?;
    }
    Ok(())
}

fn configure_claude_interactive(
    command: &mut Command,
    permission_mode: &str,
    skip_permissions: bool,
    settings_json: &str,
) {
    command.args(["--permission-mode", permission_mode]);
    if skip_permissions {
        command.arg("--dangerously-skip-permissions");
    }
    command.arg("--settings").arg(settings_json);
}

impl AgentRuntimeAdapter for ClaudeAdapter {
    fn kind(&self) -> AgentRuntimeKind {
        AgentRuntimeKind::ClaudeCode
    }
    fn execute(&self, request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult> {
        if let Some(result) = execute_preferred_acp(
            ManagedAcpKind::ClaudeCode,
            &["sessionId", "session_id"],
            request,
            cancelled,
        )? {
            return Ok(result);
        }
        let mut command = runtime_command(self.kind())?;
        configure_collaboration_mcp(&mut command, request)?;
        let (permission_mode, skip_permissions) = if request.task.id.starts_with("cli-message-") {
            ("dontAsk", false)
        } else {
            claude_permission_settings(
                request.task.workspace_mode,
                request.agent.permission_profile,
            )
        };
        if request.task.id.starts_with("cli-message-")
            || (request.task.workspace_mode == WorkspaceMode::ReadOnly
                && request_requires_session_cli(request)?)
        {
            let cli = std::env::current_exe()?.display().to_string();
            let bash_cli = cli.replace('\\', "/");
            command.arg("--allowedTools").arg(format!("Bash({cli} sessions),Bash({cli} send:*),Bash({bash_cli} sessions),Bash({bash_cli} send:*)"))
                .arg("--disallowedTools").arg("Write,Edit,NotebookEdit");
        }
        if let Some(session_id) = locator_string(
            request.resume_locator.as_ref(),
            &["sessionId", "session_id"],
        ) {
            command.arg("--resume").arg(session_id);
        } else {
            command
                .arg("--name")
                .arg(format!("CrewVela · {}", request.task.title));
        }
        let app_session = request
            .terminal_host
            .sessions
            .create_agent(NewAgentSession {
                avatar_id: request.agent.avatar_id.clone(),
                project_id: request.project.id.clone(),
                agent_id: request.agent.id.clone(),
                runtime_kind: request.agent.runtime_kind.as_str().to_string(),
                agent_name: request.agent.name.clone(),
                cwd: request.cwd.display().to_string(),
            })?;
        let hook_command = request
            .terminal_host
            .sessions
            .hook_command(&app_session.id)?;
        let environment = command
            .get_envs()
            .filter_map(|(key, value)| {
                value.map(|value| {
                    (
                        key.to_string_lossy().into_owned(),
                        value.to_string_lossy().into_owned(),
                    )
                })
            })
            .collect::<HashMap<_, _>>();
        let settings_json = crate::pty::build_settings_json(&environment, &hook_command);
        configure_claude_interactive(
            &mut command,
            permission_mode,
            skip_permissions,
            &settings_json,
        );
        command.current_dir(external_command_path(&request.cwd));
        request.terminal_host.execute_native(
            self.kind(),
            &command,
            &render_task_prompt(request),
            request,
            cancelled,
            skip_permissions,
        )
    }
}

impl AgentRuntimeAdapter for CodexAdapter {
    fn kind(&self) -> AgentRuntimeKind {
        AgentRuntimeKind::Codex
    }
    fn execute(&self, request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult> {
        if let Some(result) = execute_preferred_acp(
            ManagedAcpKind::Codex,
            &["sessionId", "threadId", "thread_id"],
            request,
            cancelled,
        )? {
            return Ok(result);
        }
        let mut command = runtime_command(self.kind())?;
        configure_codex_base(&mut command, request)?;
        command
            .arg("--no-alt-screen")
            .arg("--dangerously-bypass-hook-trust");
        let app_session = request
            .terminal_host
            .sessions
            .create_agent(NewAgentSession {
                avatar_id: request.agent.avatar_id.clone(),
                project_id: request.project.id.clone(),
                agent_id: request.agent.id.clone(),
                runtime_kind: request.agent.runtime_kind.as_str().to_string(),
                agent_name: request.agent.name.clone(),
                cwd: request.cwd.display().to_string(),
            })?;
        let notify = request
            .terminal_host
            .sessions
            .agent_turn_hook_command(&app_session.id)?;
        command
            .arg("-c")
            .arg(format!("notify={}", serde_json::to_string(&notify)?));
        if let Some(thread_id) =
            locator_string(request.resume_locator.as_ref(), &["threadId", "thread_id"])
        {
            command.arg("resume").arg(thread_id);
        }
        command.current_dir(external_command_path(&request.cwd));
        let prompt = render_task_prompt(request);
        match request.terminal_host.execute_native(
            self.kind(),
            &command,
            &prompt,
            request,
            cancelled,
            request.agent.permission_profile == PermissionProfile::Yolo,
        ) {
            Ok(result) => Ok(result),
            Err(error) if codex_native_startup_failure(&error) => {
                let _ = request.store.append_run_log(
                    &request.run_id,
                    "system",
                    &format!("Codex 原生 TUI 未就绪，切换到可视化 exec 协议继续执行：{error}"),
                );
                let mut fallback = runtime_command(self.kind())?;
                configure_codex_base(&mut fallback, request)?;
                fallback.args(["exec", "--json", "-"]);
                execute_command(self.kind(), fallback, Some(prompt), request, cancelled)
            }
            Err(error) => Err(error),
        }
    }
}

impl AgentRuntimeAdapter for KimiCodeAdapter {
    fn kind(&self) -> AgentRuntimeKind {
        AgentRuntimeKind::KimiCode
    }

    fn execute(&self, request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult> {
        let _mcp_config = KimiMcpConfigGuard::install(request)?;
        let mut command = runtime_command(self.kind())?;
        configure_collaboration_mcp(&mut command, request)?;
        command.args(["--output-format", "stream-json"]);
        // Kimi Code 的 `-p` 非交互模式自身使用 auto 权限；官方 CLI 明确拒绝
        // 与 `--auto`、`--yolo` 或 `--plan` 组合。工作区约束由任务提示和
        // CrewVela 的 Worktree/共享写入调度边界承担。
        if let Some(session_id) = locator_string(
            request.resume_locator.as_ref(),
            &["sessionId", "session_id"],
        ) {
            command.arg("--session").arg(session_id);
        }
        command.arg("-p").arg(render_task_prompt(request));
        execute_command(self.kind(), command, None, request, cancelled)
    }
}

/// Kimi Code 从工作目录的 `.kimi-code/mcp.json` 读取项目级 MCP。
/// 运行期间合并 CrewVela 服务，完成后恢复用户原文件，避免污染仓库配置。
struct KimiMcpConfigGuard {
    path: PathBuf,
    previous: Option<Vec<u8>>,
    injected: Vec<u8>,
    _lease: KimiMcpLease,
}
static KIMI_MCP_PATHS: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
struct KimiMcpLease(PathBuf);
impl Drop for KimiMcpLease {
    fn drop(&mut self) { if let Ok(mut paths) = KIMI_MCP_PATHS.lock() { paths.retain(|path| path != &self.0); } }
}

impl KimiMcpConfigGuard {
    fn install(request: &RuntimeRequest) -> Result<Self> {
        Self::install_at(&request.cwd, &request.collaboration.endpoint)
    }

    fn install_at(cwd: &std::path::Path, endpoint: &str) -> Result<Self> {
        let directory = cwd.canonicalize()?.join(".kimi-code");
        let path = directory.join("mcp.json");
        let lease = {
            let mut paths = KIMI_MCP_PATHS.lock().map_err(|_| anyhow!("Kimi MCP 路径锁损坏"))?;
            if paths.contains(&path) { anyhow::bail!("此目录已有 Kimi 任务正在使用项目 MCP 配置，请等待完成：{}", path.display()); }
            paths.push(path.clone());
            KimiMcpLease(path.clone())
        };
        let previous =
            if path.exists() {
                Some(std::fs::read(&path).with_context(|| {
                    format!("读取 Kimi Code 项目 MCP 配置失败：{}", path.display())
                })?)
            } else {
                None
            };
        let mut config = match previous.as_deref() {
            Some(bytes) => serde_json::from_slice::<serde_json::Value>(bytes)
                .with_context(|| format!("解析 Kimi Code 项目 MCP 配置失败：{}", path.display()))?,
            None => serde_json::json!({}),
        };
        let root = config
            .as_object_mut()
            .ok_or_else(|| anyhow!("Kimi Code 项目 MCP 配置必须是 JSON 对象"))?;
        let servers = root
            .entry("mcpServers")
            .or_insert_with(|| serde_json::json!({}))
            .as_object_mut()
            .ok_or_else(|| anyhow!("Kimi Code mcpServers 必须是 JSON 对象"))?;
        servers.insert(
            "crewvela".to_string(),
            serde_json::json!({
                "url": format!("{endpoint}/mcp"),
                "bearerTokenEnvVar": "CREWVELA_AGENT_TOKEN"
            }),
        );
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("创建 Kimi Code 项目配置目录失败：{}", directory.display()))?;
        let injected = serde_json::to_vec_pretty(&config)?;
        std::fs::write(&path, &injected)
            .with_context(|| format!("写入 Kimi Code 项目 MCP 配置失败：{}", path.display()))?;
        Ok(Self { path, previous, injected, _lease: lease })
    }
}

impl Drop for KimiMcpConfigGuard {
    fn drop(&mut self) {
        let current = std::fs::read(&self.path);
        if !current.as_ref().is_ok_and(|bytes| bytes == &self.injected) {
            // Preserve outside edits, including deletion. Keep a separate recovery copy.
            let recovery = self.path.with_extension(format!("crewvela-restore-{}.json", uuid::Uuid::new_v4()));
            if let Some(previous) = &self.previous {
                let backup_result = (|| -> std::io::Result<()> {
                    let mut options = std::fs::OpenOptions::new();
                    options.write(true).create_new(true);
                    #[cfg(unix)]
                    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
                    let mut file = options.open(&recovery)?;
                    std::io::Write::write_all(&mut file, previous)?;
                    file.sync_all()
                })();
                if let Err(error) = backup_result {
                    eprintln!("[crewvela] Kimi MCP 配置已被外部修改，保留外部版本；恢复备份写入失败 {}: {error}", recovery.display());
                } else {
                    eprintln!("[crewvela] Kimi MCP 配置已被外部修改，保留外部版本；原配置备份：{}", recovery.display());
                }
            } else {
                eprintln!("[crewvela] Kimi MCP 配置已被外部修改，保留外部版本，请手动检查 crewvela 条目：{}", self.path.display());
            }
            return;
        }
        let result = if let Some(previous) = self.previous.as_deref() {
            std::fs::write(&self.path, previous)
        } else {
            std::fs::remove_file(&self.path)
        };
        if let Err(error) = result { eprintln!("[crewvela] 恢复 Kimi MCP 配置失败 {}: {error}", self.path.display()); }
    }
}

impl AgentRuntimeAdapter for PiAdapter {
    fn kind(&self) -> AgentRuntimeKind {
        AgentRuntimeKind::Pi
    }
    fn execute(&self, request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult> {
        let session = locator_string(request.resume_locator.as_ref(), &["sessionPath"])
            .map(PathBuf::from)
            .unwrap_or(
                crate::providers::config_dir()?
                    .join("multi-agent-sessions")
                    .join(format!("{}.jsonl", request.run_id)),
            );
        if let Some(parent) = session.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if !session.exists() {
            std::fs::File::create(&session)?;
        }
        let mut command = runtime_command(self.kind())?;
        if super::conversation::is_conversation_task(&request.task.id) {
            command.args(["--no-tools", "--no-extensions", "--no-mcp", "--no-context-files", "--no-skills", "--no-prompt-templates"]);
            command.args(["--print", "--mode", "json", "--session"]).arg(&session);
            return execute_command(self.kind(), command, Some(render_task_prompt(request)), request, cancelled);
        }
        configure_collaboration_mcp(&mut command, request)?;
        command.arg("--session").arg(&session);
        command.current_dir(external_command_path(&request.cwd));
        let mut result = request.terminal_host.execute_native(
            self.kind(),
            &command,
            &render_task_prompt(request),
            request,
            cancelled,
            false,
        )?;
        let app_session = request
            .terminal_host
            .sessions
            .create_agent(NewAgentSession {
                avatar_id: request.agent.avatar_id.clone(),
                project_id: request.project.id.clone(),
                agent_id: request.agent.id.clone(),
                runtime_kind: self.kind().as_str().to_string(),
                agent_name: request.agent.name.clone(),
                cwd: request.cwd.display().to_string(),
            })?;
        let mut resume = Command::new("pi");
        request
            .terminal_host
            .sessions
            .configure_runtime_resume(&mut resume, &app_session)?;
        let actual_session = resume.get_args().nth(1).context("Pi 未报告原生会话文件")?;
        if let Some(object) = result.session_locator.as_object_mut() {
            object.insert("sessionPath".to_string(), serde_json::json!(actual_session));
        }
        Ok(result)
    }
}

impl AgentRuntimeAdapter for OpenCodeAdapter {
    fn kind(&self) -> AgentRuntimeKind {
        AgentRuntimeKind::OpenCode
    }
    fn execute(&self, request: &RuntimeRequest, cancelled: &AtomicBool) -> Result<RuntimeResult> {
        let mut command = runtime_command(self.kind())?;
        command.args(["run", "--format", "json"]);
        let dangerous_permissions = uses_dangerous_runtime_permissions(
            request.task.workspace_mode,
            request.agent.permission_profile,
        );
        if dangerous_permissions && request.agent.permission_profile == PermissionProfile::Auto {
            require_isolated_auto_approval(request, "OpenCode")?;
        }
        if dangerous_permissions {
            command.arg("--dangerously-skip-permissions");
        }
        command.arg("--dir").arg(&request.cwd);
        if let Some(session_id) = locator_string(
            request.resume_locator.as_ref(),
            &["sessionId", "sessionID", "session_id"],
        ) {
            command.arg("--session").arg(session_id);
        }
        let prompt_file = RuntimePromptFile::create(request, &render_task_prompt(request))?;
        command.arg("--file").arg(&prompt_file.path);
        // 提示语必须走 stdin：opencode 的 --file 是数组选项，会把后面的位置参数
        // 一并吞为附件文件，报 "File not found: <提示语>"。
        let result = execute_command(
            self.kind(),
            command,
            Some("请严格执行附件中的 CrewVela 任务说明。".to_string()),
            request,
            cancelled,
        );
        drop(prompt_file);
        result
    }
}

struct RuntimePromptFile {
    path: PathBuf,
}

impl RuntimePromptFile {
    fn create(request: &RuntimeRequest, prompt: &str) -> Result<Self> {
        let directory = runtime_prompt_directory()?;
        std::fs::create_dir_all(&directory)
            .with_context(|| format!("创建临时任务说明目录失败：{}", directory.display()))?;
        let path = directory.join(format!("{}.md", request.run_id));
        std::fs::write(&path, prompt)
            .with_context(|| format!("写入临时任务说明失败：{}", path.display()))?;
        Ok(Self { path })
    }
}

impl Drop for RuntimePromptFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn runtime_prompt_directory() -> Result<PathBuf> {
    Ok(crate::providers::config_dir()?.join("multi-agent-prompts"))
}

fn cleanup_stale_runtime_prompts() {
    let Ok(directory) = runtime_prompt_directory() else {
        return;
    };
    let now = std::time::SystemTime::now();
    for candidate in [directory.clone(), directory.join("visible")] {
        let Ok(entries) = std::fs::read_dir(candidate) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let stale = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| now.duration_since(modified).ok())
                .is_some_and(|age| age >= STALE_PROMPT_MAX_AGE);
            let removable = path.extension().is_some_and(|extension| {
                matches!(
                    extension.to_string_lossy().as_ref(),
                    "md" | "ps1" | "txt" | "jsonl"
                )
            });
            if stale && removable {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// 配置类永久性错误标记：重试必然以同样方式失败，调度器应立即终止而非退避重试。
#[derive(Debug)]
struct PermanentRunError(String);


impl std::fmt::Display for PermanentRunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for PermanentRunError {}

fn require_isolated_auto_approval(request: &RuntimeRequest, runtime: &str) -> Result<()> {
    let project_root = PathBuf::from(&request.project.root_path);
    let isolated_message = request.task.id.starts_with("cli-message-")
        && is_isolated_message_workspace(
            &crate::providers::config_dir()?,
            &request.task.id,
            &request.cwd,
            &project_root,
        );
    let isolated = request.task.workspace_mode == WorkspaceMode::ReadOnly
        || (request.task.workspace_mode == WorkspaceMode::Worktree && request.cwd != project_root)
        || isolated_message;
    if isolated {
        Ok(())
    } else {
        Err(PermanentRunError(format!(
            "{runtime} 的 Auto 仅允许只读或独立 Git Worktree；当前目录无法隔离，请改用 Default，或明确选择 YOLO"
        ))
        .into())
    }
}

fn execute_command(
    kind: AgentRuntimeKind,
    mut command: Command,
    stdin_payload: Option<String>,
    request: &RuntimeRequest,
    cancelled: &AtomicBool,
) -> Result<RuntimeResult> {
    command.current_dir(external_command_path(&request.cwd));
    let visible = request.terminal_host.execute(
        kind,
        &command,
        stdin_payload.as_deref(),
        request,
        cancelled,
    )?;
    let stdout = visible.output;
    if kind == AgentRuntimeKind::Codex {
        if let Some(error) =
            codex_protocol_failure(&stdout).or_else(|| codex_known_runtime_failure(&stdout))
        {
            return Err(anyhow!("{} 执行失败：{error}", request.agent.name));
        }
    }
    if !visible.succeeded {
        return Err(anyhow!(
            "{} 执行失败：{}",
            request.agent.name,
            runtime_failure_message(kind, &stdout, "")
        ));
    }
    let summary = if kind == AgentRuntimeKind::Pi && super::conversation::is_conversation_task(&request.task.id) {
        conversation_pi_output(&stdout)?
    } else { normalize_output(kind, &stdout) };
    if summary.trim().is_empty() {
        return Err(anyhow!("{} 未返回可用结果", request.agent.name));
    }
    let mut session_locator = serde_json::json!({
        "ptyId": visible.pty_id,
        "cwd": request.cwd,
        "adapter": kind,
    });
    if let Some((key, value)) = extract_session_identifier(&stdout) {
        if let Some(object) = session_locator.as_object_mut() {
            object.insert(key, serde_json::Value::String(value));
        }
    }
    Ok(RuntimeResult {
        summary,
        session_locator,
    })
}

#[allow(dead_code)]
fn execute_hidden_command(
    kind: AgentRuntimeKind,
    mut command: Command,
    stdin_payload: Option<String>,
    request: &RuntimeRequest,
    cancelled: &AtomicBool,
) -> Result<RuntimeResult> {
    command
        .current_dir(external_command_path(&request.cwd))
        .stdin(if stdin_payload.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = run_piped_command(command, stdin_payload, cancelled, RUN_TIMEOUT, &request.agent.name, |stdout, stderr| (
        spawn_runtime_stream_reader(stdout, request.store.clone(), request.run_id.clone(), "stdout", kind),
        spawn_runtime_stream_reader(stderr, request.store.clone(), request.run_id.clone(), "stderr", kind),
    ))?;
    let pid = output.pid;
    let succeeded = output.status.success();
    let stdout = output.stdout; let stderr = output.stderr;
    if kind == AgentRuntimeKind::Codex {
        if let Some(error) = codex_protocol_failure(&stdout)
            .or_else(|| codex_known_runtime_failure(&stdout))
            .or_else(|| codex_known_runtime_failure(&stderr))
        {
            return Err(anyhow!("{} 执行失败：{error}", request.agent.name));
        }
    }
    if !succeeded {
        return Err(anyhow!(
            "{} 执行失败：{}",
            request.agent.name,
            runtime_failure_message(kind, &stdout, &stderr)
        ));
    }
    let summary = normalize_output(kind, &stdout);
    if summary.trim().is_empty() {
        return Err(anyhow!("{} 未返回可用结果", request.agent.name));
    }
    let mut session_locator = serde_json::json!({"pid": pid, "cwd": request.cwd, "adapter": kind});
    if let Some((key, value)) = extract_session_identifier(&stdout) {
        if let Some(object) = session_locator.as_object_mut() {
            object.insert(key, serde_json::Value::String(value));
        }
    }
    Ok(RuntimeResult {
        summary,
        session_locator,
    })
}

#[derive(Default)]
struct RuntimeStreamCapture {
    prefix: Vec<u8>,
    tail: VecDeque<u8>,
    truncated: bool,
}

impl RuntimeStreamCapture {
    fn append(&mut self, bytes: &[u8]) {
        let prefix_remaining = STREAM_CAPTURE_PREFIX_BYTES.saturating_sub(self.prefix.len());
        if prefix_remaining > 0 {
            let prefix_count = prefix_remaining.min(bytes.len());
            self.prefix.extend_from_slice(&bytes[..prefix_count]);
            if prefix_count < bytes.len() {
                self.truncated = true;
                append_bounded_tail(&mut self.tail, &bytes[prefix_count..]);
            }
        } else {
            self.truncated = true;
            append_bounded_tail(&mut self.tail, bytes);
        }
    }

    fn output(&self) -> String {
        let mut raw = self.prefix.clone();
        if self.truncated {
            raw.extend_from_slice(b"\n[... CrewVela omitted oversized runtime output ...]\n");
            raw.extend(self.tail.iter().copied());
        }
        String::from_utf8_lossy(&raw).into_owned()
    }
}

struct RuntimeStreamReader {
    completion: mpsc::Receiver<std::io::Result<()>>,
    capture: Arc<Mutex<RuntimeStreamCapture>>,
    active: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for RuntimeStreamReader {
    fn drop(&mut self) {
        self.active.store(false, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
    }
}

struct DrainedStreamOutput {
    output: String,
    timed_out: bool,
}

fn spawn_runtime_stream_reader<R: Read + Send + 'static>(
    reader: R,
    store: MultiAgentStore,
    run_id: String,
    stream: &'static str,
    kind: AgentRuntimeKind,
) -> RuntimeStreamReader {
    let capture = Arc::new(Mutex::new(RuntimeStreamCapture::default()));
    let reader_capture = capture.clone();
    let active = Arc::new(AtomicBool::new(true));
    let reader_active = active.clone();
    let (completion_sender, completion) = mpsc::sync_channel(1);
    let thread = std::thread::spawn(move || {
        let result = read_stream_live(
            reader,
            store,
            run_id,
            stream,
            kind,
            reader_capture,
            reader_active,
        );
        let _ = completion_sender.send(result);
    });
    RuntimeStreamReader {
        completion,
        capture,
        active,
        thread: Some(thread),
    }
}

fn collect_stream_output(
    mut reader: RuntimeStreamReader,
    timeout: Duration,
) -> std::io::Result<DrainedStreamOutput> {
    let timed_out = match reader.completion.recv_timeout(timeout) {
        Ok(result) => {
            join_runtime_stream_thread(&mut reader)?;
            result?;
            false
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            reader.active.store(false, Ordering::Release);
            cancel_and_reap_runtime_stream(&mut reader)?;
            true
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            join_runtime_stream_thread(&mut reader)?;
            return Err(std::io::Error::other("Runtime 输出读取线程异常退出"));
        }
    };
    let output = reader
        .capture
        .lock()
        .map_err(|_| std::io::Error::other("Runtime 输出缓存不可用"))?
        .output();
    Ok(DrainedStreamOutput { output, timed_out })
}

fn join_runtime_stream_thread(reader: &mut RuntimeStreamReader) -> std::io::Result<()> {
    let Some(thread) = reader.thread.take() else {
        return Ok(());
    };
    thread
        .join()
        .map_err(|_| std::io::Error::other("Runtime 输出读取线程异常退出"))
}

fn cancel_and_reap_runtime_stream(reader: &mut RuntimeStreamReader) -> std::io::Result<()> {
    if reader.thread.is_none() {
        return Ok(());
    }
    let deadline = Instant::now() + STREAM_CANCEL_TIMEOUT;
    loop {
        cancel_synchronous_stream_read(reader);
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "取消 Runtime 输出读取超时",
            ));
        }
        match reader
            .completion
            .recv_timeout(remaining.min(Duration::from_millis(25)))
        {
            Ok(_) => return join_runtime_stream_thread(reader),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return join_runtime_stream_thread(reader);
            }
        }
    }
}

#[cfg(target_os = "windows")]
fn cancel_synchronous_stream_read(reader: &RuntimeStreamReader) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::IO::CancelSynchronousIo;

    if let Some(thread) = reader.thread.as_ref() {
        unsafe {
            let _ = CancelSynchronousIo(thread.as_raw_handle());
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn cancel_synchronous_stream_read(_reader: &RuntimeStreamReader) {}

fn read_stream_live<R: Read>(
    reader: R,
    store: MultiAgentStore,
    run_id: String,
    stream: &'static str,
    kind: AgentRuntimeKind,
    capture: Arc<Mutex<RuntimeStreamCapture>>,
    active: Arc<AtomicBool>,
) -> std::io::Result<()> {
    let mut reader = BufReader::new(reader);
    loop {
        if !active.load(Ordering::Acquire) {
            break;
        }
        let event = read_bounded_event(&mut reader, |bytes| {
            capture
                .lock()
                .map_err(|_| std::io::Error::other("Runtime 输出缓存不可用"))?
                .append(bytes);
            Ok(())
        });
        let Some((line, event_truncated)) = (match event {
            Ok(event) => event,
            Err(_) if !active.load(Ordering::Acquire) => break,
            Err(error) => return Err(error),
        }) else {
            break;
        };
        if !active.load(Ordering::Acquire) {
            break;
        }
        if event_truncated {
            let _ = store.append_run_log(
                &run_id,
                "system",
                "单条 Runtime 事件超过 1 MiB，已省略该事件以保护内存",
            );
        } else {
            let decoded = String::from_utf8_lossy(&line);
            if let Some(display) = present_runtime_log(kind, &decoded, stream) {
                let _ = store.append_run_log(&run_id, stream, &display);
            }
        }
    }
    Ok(())
}

fn read_bounded_event<R: BufRead, F: FnMut(&[u8]) -> std::io::Result<()>>(
    reader: &mut R,
    mut capture: F,
) -> std::io::Result<Option<(Vec<u8>, bool)>> {
    let mut event = Vec::new();
    let mut truncated = false;
    let mut saw_bytes = false;
    loop {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            return Ok(saw_bytes.then_some((event, truncated)));
        }
        saw_bytes = true;
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        capture(&available[..consumed])?;
        let remaining = RUNTIME_EVENT_MAX_BYTES.saturating_sub(event.len());
        let copied = remaining.min(consumed);
        event.extend_from_slice(&available[..copied]);
        if copied < consumed {
            truncated = true;
        }
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(Some((event, truncated)));
        }
    }
}

fn append_bounded_tail(tail: &mut VecDeque<u8>, bytes: &[u8]) {
    tail.extend(bytes.iter().copied());
    let overflow = tail.len().saturating_sub(STREAM_CAPTURE_TAIL_BYTES);
    if overflow > 0 {
        tail.drain(..overflow);
    }
}

fn present_runtime_log(kind: AgentRuntimeKind, raw: &str, stream: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || (kind == AgentRuntimeKind::Codex && trimmed == "Reading additional input from stdin...")
    {
        return None;
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(trimmed) {
        let contains_hidden = contains_hidden_reasoning(&value);
        let mut texts = Vec::new();
        collect_output_text(&value, &mut texts);
        texts.retain(|text| !text.trim().is_empty());
        texts.dedup();
        let mut actions = Vec::new();
        collect_runtime_actions(&value, &mut actions);
        actions.extend(texts);
        actions.dedup();
        if !actions.is_empty() {
            return Some(actions.join("\n"));
        }
        if contains_hidden {
            return None;
        }
        let event_type = value
            .get("type")
            .and_then(serde_json::Value::as_str)
            .or_else(|| value.get("event").and_then(serde_json::Value::as_str));
        return event_type
            .filter(|event| !matches!(*event, "turn.completed" | "result"))
            .map(|event| format!("[{event}]"));
    }
    let prefix = if stream == "stderr" { "错误：" } else { "" };
    Some(format!("{prefix}{trimmed}"))
}

fn contains_hidden_reasoning(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Object(map) => {
            let hidden_type = map
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| {
                    matches!(
                        kind,
                        "reasoning" | "thinking" | "analysis" | "chain_of_thought"
                    )
                });
            hidden_type
                || map.contains_key("reasoning_content")
                || map.contains_key("thinking")
                || map.values().any(contains_hidden_reasoning)
        }
        serde_json::Value::Array(items) => items.iter().any(contains_hidden_reasoning),
        _ => false,
    }
}

fn collect_runtime_actions(value: &serde_json::Value, output: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            let item_type = map.get("type").and_then(serde_json::Value::as_str);
            if item_type.is_some_and(|kind| {
                matches!(
                    kind,
                    "reasoning" | "thinking" | "analysis" | "chain_of_thought"
                )
            }) {
                return;
            }
            if matches!(
                item_type,
                Some("tool_use" | "command_execution" | "mcp_tool_call")
            ) {
                let name = map
                    .get("name")
                    .or_else(|| map.get("tool"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("命令");
                let detail = map
                    .get("command")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| {
                        map.get("input")
                            .and_then(|input| input.get("command"))
                            .and_then(serde_json::Value::as_str)
                    })
                    .or_else(|| {
                        map.get("input")
                            .and_then(|input| input.get("path"))
                            .and_then(serde_json::Value::as_str)
                    });
                output.push(match detail {
                    Some(detail) => format!("调用 {name}：{detail}"),
                    None => format!("调用 {name}"),
                });
            }
            map.values()
                .for_each(|value| collect_runtime_actions(value, output));
        }
        serde_json::Value::Array(items) => items
            .iter()
            .for_each(|value| collect_runtime_actions(value, output)),
        _ => {}
    }
}

fn runtime_failure_message(kind: AgentRuntimeKind, stdout: &str, stderr: &str) -> String {
    if kind == AgentRuntimeKind::Codex {
        if let Some(error) = codex_protocol_failure(stdout)
            .or_else(|| codex_known_runtime_failure(stdout))
            .or_else(|| codex_known_runtime_failure(stderr))
        {
            return error;
        }
    }
    let stderr = stderr
        .lines()
        .filter(|line| line.trim() != "Reading additional input from stdin...")
        .collect::<Vec<_>>()
        .join("\n");
    if !stderr.trim().is_empty() {
        return stderr.trim().to_string();
    }
    let stdout = normalize_output(kind, stdout);
    if !stdout.trim().is_empty() {
        stdout.trim().to_string()
    } else {
        "进程异常退出，未返回错误详情".to_string()
    }
}

fn codex_protocol_failure(raw: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        let value = serde_json::from_str::<serde_json::Value>(line).ok()?;
        let event_type = value.get("type").and_then(serde_json::Value::as_str)?;
        if !matches!(event_type, "turn.failed" | "error") {
            return None;
        }
        first_message(&value).or_else(|| Some(format!("Codex 返回 {event_type}")))
    })
}

fn first_message(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(message) = map
                .get("message")
                .and_then(serde_json::Value::as_str)
                .filter(|message| !message.trim().is_empty())
            {
                return Some(message.trim().to_string());
            }
            map.values().find_map(first_message)
        }
        serde_json::Value::Array(items) => items.iter().find_map(first_message),
        _ => None,
    }
}

fn codex_known_runtime_failure(raw: &str) -> Option<String> {
    const MARKERS: [&str; 2] = [
        "windows sandbox failed:",
        "windows sandbox: helper_unknown_error",
    ];
    raw.lines().find_map(|line| {
        if !MARKERS.iter().any(|marker| line.contains(marker)) {
            return None;
        }
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| find_string_containing(&value, &MARKERS))
            .or_else(|| Some(line.trim().to_string()))
    })
}

fn codex_protocol_turn_completed(raw: &str) -> bool {
    raw.lines().rev().take(16).any(|line| {
        serde_json::from_str::<serde_json::Value>(line)
            .ok()
            .and_then(|value| {
                value
                    .get("type")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .is_some_and(|event_type| event_type == "turn.completed")
    }) && codex_protocol_failure(raw).is_none()
}

fn missing_pty_session_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(|cause| cause.to_string().contains("无此会话:"))
}

fn codex_native_startup_failure(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        let message = cause.to_string();
        message.contains("原生会话尚未就绪")
            || message.contains("原生会话启动失败")
            || message.contains("无此会话:")
    })
}

fn find_string_containing(value: &serde_json::Value, markers: &[&str]) -> Option<String> {
    match value {
        serde_json::Value::String(text) => markers
            .iter()
            .any(|marker| text.contains(marker))
            .then(|| text.trim().to_string()),
        serde_json::Value::Object(map) => map
            .values()
            .find_map(|value| find_string_containing(value, markers)),
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|value| find_string_containing(value, markers)),
        _ => None,
    }
}

fn locator_string<'a>(locator: Option<&'a serde_json::Value>, keys: &[&str]) -> Option<&'a str> {
    let locator = locator?;
    keys.iter()
        .find_map(|key| locator.get(*key).and_then(serde_json::Value::as_str))
}

fn extract_session_identifier(raw: &str) -> Option<(String, String)> {
    const KEYS: [&str; 6] = [
        "sessionId",
        "sessionID",
        "session_id",
        "threadId",
        "thread_id",
        "threadID",
    ];
    for line in raw.lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value.get("type").and_then(serde_json::Value::as_str) == Some("session") {
            if let Some(id) = value.get("id").and_then(serde_json::Value::as_str) {
                return Some(("sessionId".to_string(), id.to_string()));
            }
        }
        if let Some(found) = find_identifier(&value, &KEYS) {
            return Some(found);
        }
    }
    None
}

fn find_identifier(value: &serde_json::Value, keys: &[&str]) -> Option<(String, String)> {
    match value {
        serde_json::Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key).and_then(serde_json::Value::as_str) {
                    return Some(((*key).to_string(), value.to_string()));
                }
            }
            map.values().find_map(|value| find_identifier(value, keys))
        }
        serde_json::Value::Array(items) => {
            items.iter().find_map(|value| find_identifier(value, keys))
        }
        _ => None,
    }
}

fn stop_cli_checked(child: &mut Child) -> Result<()> {
    crate::acp::stop_child_checked(child).map_err(|error| anyhow!(UnverifiedStop(format!("CLI PID {} 停止未获确认：{error:#}", child.id()))))?;
    child.wait().map_err(|error| anyhow!(UnverifiedStop(format!("等待 CLI 退出失败：{error}"))))?;
    Ok(())
}


fn resolve_runtime(kind: AgentRuntimeKind) -> Result<PathBuf> {
    let name = match kind {
        AgentRuntimeKind::ClaudeCode => "claude",
        AgentRuntimeKind::Codex => "codex",
        AgentRuntimeKind::KimiCode => "kimi",
        AgentRuntimeKind::Unsupported => return Err(anyhow!("该 Runtime 已移除或不受支持，请更换 Runtime 后重试")),
        AgentRuntimeKind::MiniMaxCode => return Err(anyhow!("MiniMax CLI 已移除，请更换 Runtime 后重试")),
        AgentRuntimeKind::Pi => "pi",
        AgentRuntimeKind::OhMyPi => return Err(anyhow!("Oh My Pi 已移除，请更换 Runtime 后重试")),
        AgentRuntimeKind::OpenCode => "opencode",
        AgentRuntimeKind::ClaudeChat => {
            return Err(anyhow!("Claude Code Chat 是原生 ACP 会话，不能以 PTY 方式启动"))
        }
        AgentRuntimeKind::KimiChat => {
            return Err(anyhow!("Kimi Code Chat 是原生 ACP 会话，不能以 PTY 方式启动"))
        }
        AgentRuntimeKind::PiChat => {
            return Err(anyhow!("Pi Chat 是原生 RPC 会话，不能以 PTY 方式启动"))
        }
        AgentRuntimeKind::CodexChat => {
            return Err(anyhow!("Codex Chat 是原生 ACP 会话，不能以 PTY 方式启动"))
        }
        AgentRuntimeKind::OpenCodeChat => {
            return Err(anyhow!("OpenCode Chat 是原生 ACP 会话，不能以 PTY 方式启动"))
        }
        AgentRuntimeKind::DeepSeekHarness => "dsh",
        AgentRuntimeKind::ZCode | AgentRuntimeKind::ZCodeCli => "zcode",
    };
    super::runtime::resolve_runtime_executable(kind)
        .with_context(|| format!("未找到 {name} 命令，请先完成 Runtime 安装"))
}

fn runtime_command(kind: AgentRuntimeKind) -> Result<Command> {
    Ok(super::runtime::command_for_path(&resolve_runtime(kind)?))
}
fn session_cli_prompt(project_id: &str) -> String {
    let cli = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "crewvela".into());
    let ps = powershell_quote(&cli);
    let bash_cli = if cfg!(windows) {
        cli.replace('\\', "/")
    } else {
        cli.clone()
    };
    let bash = format!("'{}'", bash_cli.replace('\'', "'\"'\"'"));
    format!("会话通信必须使用 crewvela Skill。当前 projectId：{project_id}。先查 sessions，并按同 projectId、agentId/成员身份选择完整会话 ID；明确的本项目后台成员可用 scheduled_turn，不能改发同 Runtime 的其他项目或交互会话。保留 CREWVELA_SESSION_ID，不伪造来源。\nPowerShell（Windows；分别运行）：\n```powershell\n& {ps} sessions\n& {ps} send --to '<目标完整会话ID>' --body '完整消息'\n```\n读取环境必须用 $env:CREWVELA_CLI，不能用 Bash 的 $CREWVELA_CLI。\nBash（分别运行）：\n```bash\n{bash} sessions\n{bash} send --to '<目标完整会话ID>' --body '完整消息'\n```\n按实际 shell 选择一条命令，替换目标占位符；不要单独执行程序路径，不要把 send 写到另一个 shell 命令。禁止写 /app/outbox 或手工回复文件模拟通信。成功只代表排队，不同步轮询、不重复发送已排队消息。不对谢谢、收到或结束语再次回信。开发协作只交流接口、进度、阻塞及必要结果，并继续实际实现和验证。")
}

fn render_task_prompt(request: &RuntimeRequest) -> String {
    if super::conversation::is_conversation_task(&request.task.id) { return request.task.description.clone(); }
    if request.task.id.starts_with("cli-message-") {
        return format!("你正在处理 CrewVela 的一条会话消息，原业务任务及其状态保持不变。\n隔离消息工作目录：{}\n原项目目录（仅作背景参考）：{}\n\n{}\n\n只处理这条消息，不重跑业务任务，不创建、修改、删除或提交业务文件。需要回复时运行 CREWVELA_CLI 指向的程序的 sessions 和 send --to 子命令，按消息中的来源会话 ID 定向回复。只能运行上述 CLI 发送回复，由 CLI 自身写入通信队列；Agent 不得手工创建 outbox 或回复文件。不要仅为确认收到而反复回信，不主动创建新的往返循环。无需提交代码交付或审核决策，最后简要说明处理结果。",
            request.cwd.display(), request.project.root_path, format!("{}\n\n{}", request.task.description, session_cli_prompt(&request.project.id)));
    }
    let workspace_contract = match request.task.workspace_mode {
        WorkspaceMode::ReadOnly => {
            "只读审查：不得创建、修改、删除或提交项目文件，只能读取任务所列工作目录并输出审查结论。为运行通信 CLI，Codex 可能使用隔离 scratch 作为 shell CWD；无论当前 CWD 是哪里，都请使用下方绝对工作目录读取原项目，不能写业务文件。"
        }
        WorkspaceMode::Worktree => "独立 Worktree：只在当前工作目录内完成任务，不操作主工作区。",
        WorkspaceMode::SharedWrite => {
            "共享工作区：只修改当前任务所需文件，避免覆盖其他任务的变更。"
        }
    };
    let handoff = if request.dependency_artifacts.is_empty() {
        "无前置 Agent 交付物。".to_string()
    } else {
        request
            .dependency_artifacts
            .iter()
            .map(|artifact| {
                format!(
                    "- 工作区：{}\n  摘要：{}\n  交付哈希：{}",
                    artifact.uri,
                    artifact.summary,
                    artifact.content_hash.as_deref().unwrap_or("未提供")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let instructions = if request.user_instructions.is_empty() {
        "无人工补充指令。".to_string()
    } else {
        request
            .user_instructions
            .iter()
            .map(|instruction| format!("- {instruction}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let memories = if request.shared_memories.is_empty() {
        "暂无已验证的共享记忆。".to_string()
    } else {
        request
            .shared_memories
            .iter()
            .map(|memory| {
                format!(
                    "- [{}] {}：{}（置信度 {:.2}）",
                    memory.namespace, memory.key, memory.value, memory.confidence
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let feedback = if request.collaboration_messages.is_empty() {
        "暂无协作反馈。".to_string()
    } else {
        request
            .collaboration_messages
            .iter()
            .rev()
            .take(20)
            .rev()
            .map(|message| {
                format!(
                    "- [{} → {}] {}",
                    message.sender_type,
                    message.target_agent_id.as_deref().unwrap_or("项目"),
                    message.body
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let review_contract = if request.agent.role == AgentRole::Reviewer
        && !request.task.id.starts_with("cli-message-")
    {
        if CollaborationGovernance::new(request.store.clone())
            .is_watchdog_task(&request.task.id)
            .unwrap_or(false)
        {
            "\n\n语义看门狗强制约束：这是整棵任务树停止后的独立复核，不要求当前任务存在直接依赖。必须检查任务说明中的全部状态与证据，并在结束前且只能调用一次 cc_review_submit，target_task_id 必须使用任务说明指定的值。整体目标有充分证据达成时提交 verdict=approved；存在遗漏、假完成、失败或阻塞时提交 verdict=changes_requested，并写清证据和可执行修复建议。未提交结构化结论，本次 Run 将失败并重试。"
        } else {
            "\n\nReviewer 强制约束：先调用 cc_task_read 确认直接依赖的被审核任务；结束前必须且只能调用一次 cc_review_submit。代码、测试和验收标准全部通过时提交 verdict=approved；存在必须修复的问题时提交 verdict=changes_requested，并在 summary 中写清问题、证据和修复建议。未提交结构化结论，本次 Run 将失败并重试。"
        }
    } else {
        ""
    };
    let collaboration = format!(
        "{}\n\n{}",
        request.collaboration.prompt_contract(),
        session_cli_prompt(&request.project.id)
    );
    // 工作风格段：人格只约束沟通风格与工作倾向；custom 且有自由文本时用自由文本。
    let persona_text = match request.agent.persona {
        AgentPersona::Custom if !request.agent.persona_custom.trim().is_empty() => {
            request.agent.persona_custom.trim().to_string()
        }
        other => other.prompt_segment().to_string(),
    };
    let persona_line = if persona_text.is_empty() {
        String::new()
    } else {
        format!("工作风格：{persona_text}\n")
    };
    format!(
        "你是 CrewVela 项目协作中的 {role}。\n{persona_line}项目目标：{goal}\n当前任务：{title}\n本轮职责以当前任务说明为准，优先于成员名称或模板职责；不能因旧模板称为前端工程师就接管另一成员的前端任务。\n任务说明：{description}\n验收标准：{acceptance}\n工作目录：{cwd}\n工作区约束：{workspace_contract}\n\n前置 Agent 交付：\n{handoff}\n\n协作反馈：\n{feedback}\n\n已验证的共享记忆：\n{memories}\n\n人工补充指令：\n{instructions}\n\n{collaboration}{review_contract}\n\n只处理当前任务，不越过依赖或擅自合并其他分支。需要前置结果时以上述持久化交付为准。Review 任务必须检查实际代码、测试结果和前置交付哈希，并明确列出问题、严重级别和修复建议。重要发现可写入 memory.put，但只有本 Run 最终验收成功后才会晋升为共享记忆。完成后给出：变更摘要、验证结果、遗留风险。若无法完成，明确说明阻塞原因。",
        role = request.agent.description,
        persona_line = persona_line,
        goal = request.project.goal,
        title = request.task.title,
        description = request.task.description,
        acceptance = request.task.acceptance_criteria,
        cwd = request.cwd.display(),
        workspace_contract = workspace_contract,
        handoff = handoff,
        memories = memories,
        feedback = feedback,
        instructions = instructions,
        collaboration = collaboration,
        review_contract = review_contract,
    )
}

pub(super) fn conversation_pi_output(raw: &str) -> Result<String> {
    let mut replies = Vec::new();
    for line in raw.lines() {
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else { continue; };
        if event.get("type").and_then(serde_json::Value::as_str) != Some("message_end") { continue; }
        let Some(message) = event.get("message").filter(|m|m.get("role").and_then(serde_json::Value::as_str)==Some("assistant")) else { continue; };
        if matches!(message.get("stopReason").and_then(serde_json::Value::as_str),Some("error"|"aborted")) {
            return Err(anyhow!("Pi 讨论回复失败：{}",message.get("errorMessage").and_then(serde_json::Value::as_str).unwrap_or("本轮中止")));
        }
        if let Some(content) = message.get("content").and_then(serde_json::Value::as_array) {
            let text = content.iter().filter(|block|block.get("type").and_then(serde_json::Value::as_str)==Some("text"))
                .filter_map(|block|block.get("text").and_then(serde_json::Value::as_str)).collect::<Vec<_>>().join("");
            if !text.trim().is_empty() { replies.push(text); }
        }
    }
    if replies.is_empty() { return Err(anyhow!("Pi 未返回可核验的 assistant message_end，公共会话不会显示终端日志或用户提示词作为回复")); }
    Ok(replies.join("\n"))
}

fn normalize_output(kind: AgentRuntimeKind, raw: &str) -> String {
    if kind == AgentRuntimeKind::ClaudeCode {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(raw.trim()) {
            if let Some(result) = value.get("result").and_then(|value| value.as_str()) {
                return result.to_string();
            }
        }
    }
    if kind == AgentRuntimeKind::KimiCode {
        return normalize_kimi_code_output(raw);
    }
    let lines = raw
        .lines()
        .flat_map(|line| {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                let mut texts = Vec::new();
                collect_output_text(&value, &mut texts);
                texts
            } else if !line.trim().is_empty() {
                vec![line.to_string()]
            } else {
                Vec::new()
            }
        })
        .collect::<Vec<_>>();
    lines.join("\n")
}

fn normalize_kimi_code_output(raw: &str) -> String {
    raw.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter(|value| value.get("role").and_then(serde_json::Value::as_str) == Some("assistant"))
        .filter_map(|value| {
            value
                .get("content")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn terminal_output_summary(raw: &str) -> String {
    let plain = strip_terminal_control_sequences(raw);
    let mut lines = plain
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if lines.len() > 80 {
        lines.drain(..lines.len() - 80);
    }
    let mut summary = lines.join("\n");
    if summary.chars().count() > 8_000 {
        summary = summary
            .chars()
            .rev()
            .take(8_000)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
    }
    summary
}

fn codex_native_prompt_ready(raw: &str) -> bool {
    // The native completion fallback calls this helper directly, so the busy
    // guard must also apply here rather than only in the interactive wrapper.
    if interactive_runtime_prompt_blocked(raw) {
        return false;
    }
    let plain = strip_terminal_control_sequences(raw);
    let lowered = plain.to_ascii_lowercase();
    // From 0.167 the startup composer is rendered before initialization is
    // complete. Its OSC idle signal, rather than the placeholder, opens input.
    let startup_uses_native_state = lowered.split_once("openai codex (v")
        .and_then(|(_, version)| version.split(')').next())
        .and_then(|version| {
            let mut parts = version.split('.');
            Some((parts.next()?.parse::<u32>().ok()?, parts.next()?.parse::<u32>().ok()?))
        }).is_some_and(|(major, minor)| major > 0 || minor >= 167);
    if startup_uses_native_state && !codex_terminal_state(raw).is_some_and(|(ready, _)| ready) {
        return false;
    }
    let lines = plain
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    let Some(composer_index) = lines
        .iter()
        .rposition(|line| line.starts_with('›') || line.starts_with('❯'))
    else {
        return false;
    };
    let blocked_after_composer = lines[composer_index..].iter().any(|line| {
        let line = line.to_ascii_lowercase();
        [
            "sign in",
            "do you trust",
            "trust the contents",
            "enter to confirm",
            "update available",
            "loading...",
        ]
        .iter()
        .any(|marker| line.contains(marker))
    });
    let legacy_ready = lowered.contains("for shortcuts");
    let compact_composer = lines[composer_index].trim_start_matches(['›', '❯']).trim()
        .eq_ignore_ascii_case("ask codex to do anything");
    let compact_loading = lines[..composer_index].iter().rev().take(2)
        .any(|line| line.eq_ignore_ascii_case("loading"));
    // The compact composer remains after a turn while the startup header is
    // gone. Requiring that header strands every second queued message.
    let current_ready = (lowered.contains("openai codex") && lowered.contains("/model to change"))
        || (compact_composer && !compact_loading);
    !blocked_after_composer && (legacy_ready || current_ready)
}

pub(crate) fn codex_native_update_prompt(raw: &str) -> bool {
    let plain = strip_terminal_control_sequences(raw).to_ascii_lowercase();
    // Cursor-based redraws may emit only fragments of the banner (e.g.
    // "Updat available"). Identify the actionable menu, not its decorative title.
    plain.contains("update now")
        && plain.contains("skip")
        && (plain.contains("press enter to continue") || plain.contains("enter continue"))
}

#[derive(Default)]
struct CodexNativeCompletionFallback {
    last_transcript_change: Option<Instant>,
}

impl CodexNativeCompletionFallback {
    fn observe(&mut self, raw: &str, transcript_changed: bool, now: Instant) -> bool {
        if transcript_changed {
            self.last_transcript_change = Some(now);
        }
        if !codex_native_turn_returned_to_composer(raw) {
            return false;
        }
        self.last_transcript_change
            .is_some_and(|changed_at| now.duration_since(changed_at) >= Duration::from_millis(1500))
    }
}

fn codex_native_turn_returned_to_composer(raw: &str) -> bool {
    if !codex_native_prompt_ready(raw) {
        return false;
    }
    let plain = strip_terminal_control_sequences(raw);
    plain
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('›') || line.starts_with('❯'))
        .take(2)
        .count()
        >= 2
}

fn submit_native_prompt(
    prompt: &str,
    mut write: impl FnMut(&[u8]) -> Result<()>,
    mut wait_until_rendered: impl FnMut() -> Result<()>,
    mut ensure_not_cancelled: impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut paste = b"\x1b[200~".to_vec();
    paste.extend_from_slice(prompt.as_bytes());
    paste.extend_from_slice(b"\x1b[201~");
    write(&paste)?;
    wait_until_rendered()?;
    ensure_not_cancelled()?;
    write(b"\r")
}

fn native_prompt_paste_rendered(prompt: &str, raw: &str) -> bool {
    let plain = strip_terminal_control_sequences(raw);
    if plain.to_ascii_lowercase().contains("[pasted text #") {
        return true;
    }
    let Some(first_line) = prompt.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return false;
    };
    let probe = first_line.chars().take(32).collect::<String>();
    !probe.is_empty() && plain.contains(&probe)
}

pub(crate) fn interactive_runtime_prompt_ready(kind: AgentRuntimeKind, raw: &str) -> bool {
    let raw = runtime_prompt_frame(raw);
    let plain = strip_terminal_control_sequences(raw);
    let lowered = plain.to_ascii_lowercase();
    // Never paste chat into a permission/login picker or an active turn. Runtime
    // redraws can retain their composer while a request is running.
    if interactive_runtime_prompt_blocked(raw) {
        return false;
    }
    if kind == AgentRuntimeKind::Codex {
        return codex_native_prompt_ready(raw);
    }
    if kind == AgentRuntimeKind::KimiCode {
        let plain = strip_terminal_control_sequences(raw);
        return plain
            .split(['\r', '\n'])
            .any(|line| line.trim_start().starts_with("│ > "));
    }
    match kind {
        AgentRuntimeKind::OpenCode => {
            lowered.contains("ask anything")
                && lowered.contains("ctrl+p")
                && lowered.contains("commands")
        }
        // ZCode's composer replaces its empty-input placeholder while busy.
        // The sidebar's "Ready." label can be hidden in narrow terminals or
        // replaced by an MCP warning, so it is not a readiness requirement.
        AgentRuntimeKind::ZCodeCli => {
            plain.lines().any(|line| line.split('│').any(|cell| matches!(cell.trim(), "Type a prompt" | "输入提示词")))
                && !["type to queue input", "输入内容会排队", "no available models", "没有可用模型",
                    "waiting for browser authorization", "正在等待浏览器授权", "enter the api key", "输入 api key"]
                    .iter().any(|marker| lowered.contains(marker))
        }
        AgentRuntimeKind::MiniMaxCode | AgentRuntimeKind::Unsupported => false,
        // These TUIs have dynamic composers. Project communication uses their
        // structured task adapters rather than guessing an idle screen.
        AgentRuntimeKind::Pi
        | AgentRuntimeKind::DeepSeekHarness
        | AgentRuntimeKind::ZCode => false,
        // Claude's SessionStart/UserPromptSubmit/Stop hooks are authoritative.
        AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::OhMyPi => false,
        _ => false,
    }
}

/// Codex 0.167 emits OSC state independently of its incrementally drawn composer.
/// Include the end offset so a later busy/dialog redraw wins over an older signal.
pub(crate) fn codex_terminal_state(raw: &str) -> Option<(bool, usize)> {
    let frame = runtime_prompt_frame(raw);
    let frame_start = raw.len() - frame.len();
    frame.match_indices("\x1b]7501;").filter_map(|(start, prefix)| {
        let message = &frame[start + prefix.len()..];
        let end = message.find('\x07').into_iter().chain(message.find("\x1b\\")).min()?;
        let fields = message[..end].split(':').collect::<Vec<_>>();
        if !fields.contains(&"app=codex") { return None; }
        let ready = match fields.first().copied()? {
            "state=idle" | "state=done" => Some(true),
            "state=working" => Some(false),
            _ => None,
        };
        let terminator_length = if message.as_bytes()[end] == 7 { 1 } else { 2 };
        Some(ready.map(|ready| (ready, frame_start + start + prefix.len() + end + terminator_length)))
    }).last().flatten()
}

/// Only a known active-turn banner can be released by a completion hook.
/// Ambiguous prompts and dialogs must wait for an observed fresh composer.
pub(crate) fn interactive_runtime_prompt_busy(raw: &str) -> bool {
    let lowered = strip_terminal_control_sequences(runtime_prompt_frame(raw)).to_ascii_lowercase();
    ["esc to interrupt", "esc to cancel", "enter to steer", "esc interrupt", "esc cancel", "enter steers"]
        .iter().any(|marker| lowered.contains(marker))
        && !["permission required", "allow once", "allow always", "sign in", "waiting for approval",
            "confirm to continue", "enter to confirm", "enter continue", "esc skip", "update available",
            "do you trust", "type password", "type secret", "enter to lock"]
            .iter().any(|marker| lowered.contains(marker))
}

pub(crate) fn interactive_runtime_prompt_blocked(raw: &str) -> bool {
    let lowered = strip_terminal_control_sequences(runtime_prompt_frame(raw)).to_ascii_lowercase();
    // Codex 0.162 keeps its empty composer visible below an active-turn banner.
    // Its position does not mean input is idle. A later completed-turn banner
    // may replace a historical busy line in an accumulated terminal frame.
    if lowered.contains("ask codex to do anything") {
        let active = ["esc to interrupt", "esc to cancel", "enter to steer"]
            .iter().filter_map(|marker| lowered.rfind(marker)).max();
        let completed = lowered.rfind("worked for");
        if active.is_some_and(|position| completed.is_none_or(|done| position > done)) {
            return true;
        }
    }
    let composer = [
        "│ > ",
        "› ",
        "❯ ",
        "ask anything",
        "type a prompt",
        "输入提示词",
    ]
    .iter()
    .filter_map(|marker| lowered.rfind(marker))
    .max();
    let blocker = [
        "esc to cancel",
        "esc to interrupt",
        "esc interrupt",
        "esc cancel",
        "enter to steer",
        "enter steers",
        "enter steer",
        "escape to interrupt",
        "do you trust",
        "permission required",
        "allow once",
        "allow always",
        "sign in to continue",
        "waiting for approval",
        "confirm to continue",
        "loading...",
        "msg=interrupt",
        "发消息=中断",
        "processing command...",
        "正在处理命令",
        "input temporarily disabled",
        "输入已暂时禁用",
        "type password",
        "type secret",
        "enter to lock",
        "enter to confirm",
        "enter continue",
        "esc skip",
        "updating codex via",
    ]
    .iter()
    .filter_map(|marker| lowered.rfind(marker))
    .max();
    blocker.is_some_and(|position| composer.is_none_or(|ready_position| position > ready_position))
}

fn runtime_prompt_frame(raw: &str) -> &str {
    let start = ["\x1b[2J", "\x1b[H", "\x1b[1;1H"]
        .iter()
        .filter_map(|marker| raw.rfind(marker))
        .max()
        .unwrap_or(0);
    &raw[start..]
}

pub(crate) fn strip_terminal_control_sequences(raw: &str) -> String {
    let mut output = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(character) = chars.next() {
        if character != '\u{1b}' {
            if !character.is_control() || matches!(character, '\n' | '\r' | '\t') {
                output.push(character);
            }
            continue;
        }
        match chars.next() {
            Some('[') => {
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                let mut previous_escape = false;
                for next in chars.by_ref() {
                    if next == '\u{7}' || (previous_escape && next == '\\') {
                        break;
                    }
                    previous_escape = next == '\u{1b}';
                }
            }
            Some(_) | None => {}
        }
    }
    output
}

fn collect_output_text(value: &serde_json::Value, output: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if map
                .get("type")
                .and_then(serde_json::Value::as_str)
                .is_some_and(|kind| {
                    matches!(
                        kind,
                        "reasoning" | "thinking" | "analysis" | "chain_of_thought"
                    )
                })
            {
                return;
            }
            for (key, value) in map {
                if matches!(key.as_str(), "reasoning_content" | "thinking") {
                    continue;
                }
                if matches!(
                    key.as_str(),
                    "text" | "content" | "message" | "result" | "output"
                ) {
                    if let Some(text) = value.as_str().filter(|text| !text.trim().is_empty()) {
                        if !output.iter().any(|existing| existing == text) {
                            output.push(text.to_string());
                        }
                        continue;
                    }
                }
                collect_output_text(value, output);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_output_text(item, output);
            }
        }
        _ => {}
    }
}

fn prepare_workspace(
    project: &MultiAgentProject,
    task: &MultiAgentTask,
    run_id: &str,
    dependency_artifacts: &[ArtifactRecord],
) -> Result<PathBuf> {
    let worktrees_root = crate::providers::config_dir()?.join("worktrees");
    prepare_workspace_in(&worktrees_root, project, task, run_id, dependency_artifacts)
}

fn prepare_workspace_in(
    worktrees_root: &std::path::Path,
    project: &MultiAgentProject,
    task: &MultiAgentTask,
    run_id: &str,
    dependency_artifacts: &[ArtifactRecord],
) -> Result<PathBuf> {
    let root = PathBuf::from(&project.root_path);
    if task.workspace_mode == WorkspaceMode::SharedWrite {
        return Ok(root);
    }
    if !is_git_worktree(&root) {
        if matches!(task.workspace_mode, WorkspaceMode::Worktree | WorkspaceMode::ReadOnly) {
            return Err(anyhow!("任务要求 Worktree/只读隔离，但项目目录不是 Git 仓库；请先初始化并提交 Git 仓库，或明确改用共享目录模式"));
        }
        return Ok(root);
    }
    let (repository_root, project_relative) = git_repository_layout(&root)?;
    let path = worktrees_root.join(&project.id).join(&task.id).join(run_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut command = hidden_command("git");
    command
        .arg("-C")
        .arg(&repository_root)
        .args(["worktree", "add"]);
    if task.workspace_mode == WorkspaceMode::ReadOnly {
        command.arg("--detach");
    } else {
        let short = run_id.chars().take(8).collect::<String>();
        let suffix = format!("{}-{}", task.id.chars().take(8).collect::<String>(), short);
        command
            .arg("-b")
            .arg(crate::git_source_control::managed_branch_name(
                "task", &suffix,
            ));
    }
    let output = command
        .arg(&path)
        .arg("HEAD")
        .output()
        .context("创建任务 Worktree 失败")?;
    if !output.status.success() {
        return Err(anyhow!(
            "创建任务 Worktree 失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let workspace = path.join(project_relative);
    std::fs::create_dir_all(&workspace)?;
    if let Err(integration_error) = integrate_dependency_commits(&workspace, dependency_artifacts) {
        let cleanup = hidden_command("git")
            .arg("-C")
            .arg(&repository_root)
            .args(["worktree", "remove", "--force"])
            .arg(&path)
            .output()
            .context("清理失败的任务 Worktree 时无法启动 Git")?;
        if !cleanup.status.success() {
            return Err(anyhow!(
                "{integration_error}；清理失败的任务 Worktree 也失败：{}",
                String::from_utf8_lossy(&cleanup.stderr).trim()
            ));
        }
        return Err(integration_error);
    }
    Ok(workspace)
}

fn integrate_dependency_commits(
    workspace: &std::path::Path,
    dependency_artifacts: &[ArtifactRecord],
) -> Result<()> {
    let base_head = git_head(workspace)?;
    let mut commits = Vec::new();
    for artifact in dependency_artifacts {
        if artifact.artifact_type != "git_commit" {
            continue;
        }
        let commit = artifact
            .content_hash
            .as_deref()
            .filter(|hash| valid_commit_hash(hash))
            .ok_or_else(|| anyhow!("前置 Git 交付缺少有效的持久化提交哈希：{}", artifact.id))?
            .to_string();
        if !git_commit_exists(workspace, &commit) {
            return Err(anyhow!(
                "前置 Git 交付提交不存在或无法验证：{}（交付 {}）",
                commit,
                artifact.id
            ));
        }
        if commit != base_head && !commits.iter().any(|existing| existing == &commit) {
            commits.push(commit);
        }
    }
    let mut processed_artifact_commits = Vec::new();
    for artifact_commit in commits {
        for commit in
            commits_not_in_workspace(workspace, &artifact_commit, &processed_artifact_commits)?
        {
            let output = hidden_command("git")
                .arg("-C")
                .arg(workspace)
                .args(["-c", "user.name=CrewVela", "-c", "user.email=crewvela@local"])
                .args(["cherry-pick", &commit])
                .output()
                .with_context(|| format!("集成前置交付 {commit} 失败"))?;
            if !output.status.success() {
                let _ = hidden_command("git")
                    .arg("-C")
                    .arg(workspace)
                    .args(["cherry-pick", "--abort"])
                    .status();
                return Err(anyhow!(
                    "集成前置交付 {commit} 发生冲突：{}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ));
            }
        }
        processed_artifact_commits.push(artifact_commit);
    }
    Ok(())
}

fn valid_commit_hash(commit: &str) -> bool {
    matches!(commit.len(), 40 | 64)
        && commit
            .chars()
            .all(|character| character.is_ascii_hexdigit())
}

fn git_commit_exists(workspace: &std::path::Path, commit: &str) -> bool {
    hidden_command("git")
        .arg("-C")
        .arg(workspace)
        .args(["cat-file", "-e", &format!("{commit}^{{commit}}")])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn commits_not_in_workspace(
    workspace: &std::path::Path,
    commit: &str,
    excluded_artifact_commits: &[String],
) -> Result<Vec<String>> {
    let mut command = hidden_command("git");
    command.arg("-C").arg(workspace).args([
        "rev-list",
        "--reverse",
        "--topo-order",
        "--no-merges",
        commit,
        "^HEAD",
    ]);
    for excluded in excluded_artifact_commits {
        command.arg(format!("^{excluded}"));
    }
    let output = command
        .output()
        .with_context(|| format!("读取前置交付提交链失败：{commit}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "读取前置交付提交链失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect())
}

fn git_head(workspace: &std::path::Path) -> Result<String> {
    let output = hidden_command("git")
        .arg("-C")
        .arg(workspace)
        .args(["rev-parse", "HEAD"])
        .output()
        .with_context(|| format!("读取 Git 提交失败：{}", workspace.display()))?;
    if !output.status.success() {
        return Err(anyhow!(
            "读取 Git 提交失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn git_repository_layout(root: &std::path::Path) -> Result<(PathBuf, PathBuf)> {
    let project_root = std::fs::canonicalize(root)
        .with_context(|| format!("解析项目目录失败：{}", root.display()))?;
    let output = hidden_command("git")
        .arg("-C")
        .arg(&project_root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("读取 Git 仓库根目录失败")?;
    if !output.status.success() {
        return Err(anyhow!(
            "读取 Git 仓库根目录失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let repository_root = std::fs::canonicalize(String::from_utf8_lossy(&output.stdout).trim())
        .context("解析 Git 仓库根目录失败")?;
    let project_relative = project_root
        .strip_prefix(&repository_root)
        .map_err(|_| anyhow!("项目目录不在 Git 仓库根目录内"))?
        .to_path_buf();
    Ok((repository_root, project_relative))
}
pub(super) fn is_git_worktree(root: &std::path::Path) -> bool {
    hidden_command("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--is-inside-work-tree"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn workspace_content_hash(cwd: &std::path::Path) -> Option<String> {
    if !is_git_worktree(cwd) {
        return None;
    }
    let changed = hidden_command("git")
        .arg("-C")
        .arg(cwd)
        .args(["diff", "--name-only", "-z", "HEAD"])
        .output()
        .ok()?;
    let untracked = hidden_command("git")
        .arg("-C")
        .arg(cwd)
        .args(["ls-files", "--others", "--exclude-standard", "-z"])
        .output()
        .ok()?;
    let mut paths = changed
        .stdout
        .split(|byte| *byte == 0)
        .chain(untracked.stdout.split(|byte| *byte == 0))
        .filter(|path| !path.is_empty())
        .map(|path| String::from_utf8_lossy(path).to_string())
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if paths.is_empty() {
        let head = hidden_command("git")
            .arg("-C")
            .arg(cwd)
            .args(["rev-parse", "HEAD"])
            .output()
            .ok()?;
        head.stdout.hash(&mut hasher);
    } else {
        for relative in paths {
            relative.hash(&mut hasher);
            if let Ok(bytes) = std::fs::read(cwd.join(&relative)) {
                bytes.hash(&mut hasher);
            }
        }
    }
    Some(format!("{:016x}", hasher.finish()))
}

#[cfg(test)]
mod tests {

    #[cfg(unix)]
    #[test]
    fn posix_visible_script_rejects_capture_failure_and_bounds_output() {
        let root = std::env::temp_dir().join(format!("visible-bounded-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        for (name, payload) in [("disk-failure", "printf hello"), ("budget", "head -c 6291456 /dev/zero")] {
            let output = if name == "disk-failure" { root.clone() } else { root.join("bounded-output") };
            let sentinel = root.join(format!("{name}.exit"));
            let script = root.join(format!("{name}.sh"));
            let mut command = Command::new("/bin/bash");
            command.args(["-c", payload]);
            std::fs::write(&script, posix_runtime_script(&command, None, &output, &sentinel, "DONE:")).unwrap();
            let result = Command::new("/bin/bash").arg(script).stdout(Stdio::null()).stderr(Stdio::piped()).output().unwrap();
            assert_eq!(result.status.code(), Some(74), "{}", String::from_utf8_lossy(&result.stderr));
            assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "74");
            if name == "budget" { assert!(std::fs::metadata(output).unwrap().len() <= 5 * 1024 * 1024); }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn posix_visible_script_preserves_arguments_stdin_and_failure_status() {
        let root = std::env::temp_dir().join(format!("visible ' unicode-中文-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("input ' file");
        std::fs::write(&input, "payload 中文\n").unwrap();
        for code in [0, 7] {
            let output = root.join(format!("output-{code}"));
            let sentinel = root.join(format!("exit-{code}"));
            let script = root.join(format!("run-{code}.sh"));
            let mut command = Command::new("/bin/bash");
            command.current_dir(&root).args(["-c", "printf '%s\\n' \"$1\"; cat; exit \"$2\"", "fixture", "literal ' $HOME `whoami` 中文", &code.to_string()]);
            std::fs::write(&script, posix_runtime_script(&command, Some(&input), &output, &sentinel, "DONE:")).unwrap();
            let result = Command::new("/bin/bash").arg(&script).output().unwrap();
            assert_eq!(result.status.code(), Some(code));
            assert_eq!(std::fs::read_to_string(&sentinel).unwrap(), code.to_string());
            assert_eq!(std::fs::read_to_string(&output).unwrap(), "literal ' $HOME `whoami` 中文\npayload 中文\n");
            assert!(String::from_utf8_lossy(&result.stdout).contains(&format!("DONE:{code}")));
        }
        std::fs::remove_dir_all(root).unwrap();
    }
    use super::*;

    #[test]
    fn piped_command_drains_large_outputs_before_large_prompt_input() {
        let mut command = hidden_command("node");
        command.args(["-e", "(async()=>{const write=(s,v)=>new Promise((ok,fail)=>s.write(v,e=>e?fail(e):ok()));await write(process.stdout,'o'.repeat(2*1024*1024));await write(process.stderr,'e'.repeat(2*1024*1024));let n=0;process.stdin.on('data',b=>n+=b.length);process.stdin.on('end',()=>console.log('INPUT_BYTES='+n));process.stdin.resume()})().catch(e=>{console.error(e);process.exit(1)})"]);
        let input = "输入".repeat(400_000); let expected = input.len();
        let output = run_piped_command(command, Some(input), &AtomicBool::new(false), Duration::from_secs(10), "测试Agent", |stdout, stderr| (spawn_passive_stream_reader(stdout), spawn_passive_stream_reader(stderr))).unwrap();
        assert!(output.status.success());
        assert!(output.stdout.contains(&format!("INPUT_BYTES={expected}")));
        assert!(output.stdout.starts_with('o')); assert!(output.stderr.starts_with('e'));
        assert!(output.stderr.len() >= 2 * 1024 * 1024);
    }

    #[test]
    fn piped_command_cancel_and_deadline_cover_a_blocked_stdin_writer() {
        for cancel in [false, true] {
            let cancelled = Arc::new(AtomicBool::new(false));
            let trigger = if cancel { let flag = Arc::clone(&cancelled); Some(std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(120)); flag.store(true, Ordering::Release); })) } else { None };
            let mut command = hidden_command("node"); command.args(["-e", "process.stdin.pause();setInterval(()=>{},1000)"]);
            let started = Instant::now();
            let error = run_piped_command(command, Some("x".repeat(8 * 1024 * 1024)), &cancelled, Duration::from_millis(300), "测试Agent", |stdout, stderr| (spawn_passive_stream_reader(stdout), spawn_passive_stream_reader(stderr))).err().expect("blocked stdin must be bounded");
            assert!(error.to_string().contains(if cancel { "取消" } else { "超时" }), "{error:#}");
            assert!(error.downcast_ref::<UnverifiedStop>().is_none(), "fixture tree should stop normally: {error:#}");
            assert!(started.elapsed() < Duration::from_secs(3));
            if let Some(trigger) = trigger { trigger.join().unwrap(); }
        }
    }

    #[cfg(windows)]
    #[test]
    fn blocked_stdin_stop_failure_keeps_original_registered_identity() {
        let root = std::env::temp_dir().join(format!("piped-stop-denied-{}", uuid::Uuid::new_v4())); std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("pid.txt");
        let mut command = hidden_command("node"); command.args(["-e", "const fs=require('fs');fs.writeFileSync(process.argv[1]+'.tmp',String(process.pid));fs.renameSync(process.argv[1]+'.tmp',process.argv[1]);process.stdin.pause();setInterval(()=>{},1000)"]).arg(&marker);
        let mut refusal = None; let mut pid = 0;
        let error = run_piped_command(command, Some("x".repeat(8 * 1024 * 1024)), &AtomicBool::new(false), Duration::from_millis(700), "测试Agent", |stdout, stderr| {
            let readers = (spawn_passive_stream_reader(stdout), spawn_passive_stream_reader(stderr));
            // This setup hook runs after owned spawn, before deadline/cancel
            // observation. Install refusal before cleanup can race fixture boot.
            let started = Instant::now();
            pid = loop {
                if let Some(pid) = std::fs::read_to_string(&marker).ok().and_then(|raw| raw.parse().ok()) { break pid; }
                assert!(started.elapsed() < Duration::from_secs(10), "isolated Node fixture did not publish its PID"); std::thread::sleep(Duration::from_millis(5));
            };
            refusal = Some(crate::acp::TestStopFailureGuard::new(pid));
            readers
        }).err().expect("stop rejection must propagate");
        assert!(error.downcast_ref::<UnverifiedStop>().is_some());
        drop(refusal);
        // The std Child was dropped by the failed invocation, but the root's
        // original HANDLE remains registered; no PID reopening is needed.
        crate::acp::terminate_process_tree_checked(pid).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }


    #[test]
    fn repeated_project_start_reuses_the_existing_scheduler() {
        let root = std::env::temp_dir().join(format!("supervisor-idempotent-{}", uuid::Uuid::new_v4()));
        let store = MultiAgentStore::open_at(PathBuf::from(":memory:")).unwrap();
        let project = store.create_project(super::super::CreateProjectInput { name:"repeat".into(), goal:"reuse scheduler".into(), root_path:root.display().to_string(), default_branch:"main".into() }).unwrap();
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let supervisor = MultiAgentSupervisor::default();
        let original = Arc::new(AtomicBool::new(false));
        supervisor.projects.lock().unwrap().insert(project.id.clone(), Arc::clone(&original));
        let receipt = supervisor.start(store.clone(), project.id.clone(), AgentTerminalHost::new(PtyManager::new(), sessions.clone()), CollaborationBridge::start(store.clone()).unwrap()).unwrap();
        assert!(receipt.accepted, "an existing scheduler must not turn auto-start into a failure");
        assert!(Arc::ptr_eq(supervisor.projects.lock().unwrap().get(&project.id).unwrap(), &original));
        assert_eq!(supervisor.active_project_ids(), vec![project.id.clone()]);
        assert!(sessions.list().is_empty(), "reuse must not prepare another set of sessions");
        assert_eq!(store.project(&project.id).unwrap().status, project.status);
        assert!(store.snapshot(&project.id).unwrap().tasks.is_empty());
        drop(store); drop(sessions); std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeated_project_start_does_not_accept_a_stopping_scheduler() {
        let supervisor = MultiAgentSupervisor::default();
        let original = Arc::new(AtomicBool::new(true));
        supervisor.projects.lock().unwrap().insert("project".into(), Arc::clone(&original));
        assert!(supervisor.register_start("project", &Arc::new(AtomicBool::new(false)), false).is_err());
        assert!(Arc::ptr_eq(supervisor.projects.lock().unwrap().get("project").unwrap(), &original));
    }

    #[test]
    fn supervisor_shutdown_fences_both_registries_and_restores_after_failure() {
        let supervisor = MultiAgentSupervisor::default();
        let signal = Arc::new(AtomicBool::new(false));
        supervisor.projects.lock().unwrap().insert("project".into(), Arc::clone(&signal));
        supervisor.message_projects.lock().unwrap().insert("message".into(), signal);
        let guard = supervisor.begin_shutdown().unwrap();
        assert_eq!(guard.project_ids, ["message", "project"]);
        assert!(supervisor.begin_shutdown().is_err());
        assert!(supervisor.shutting_down.load(Ordering::Acquire));
        let started = Arc::new(std::sync::Barrier::new(2));
        let worker = { let supervisor = supervisor.clone(); let started = started.clone(); std::thread::spawn(move || {
            started.wait();
            for message in [false, true] {
                assert!(supervisor.register_start("late", &Arc::new(AtomicBool::new(false)), message).is_err());
            }
            assert_eq!(supervisor.active_project_ids(), ["message", "project"]);
        }) };
        started.wait(); worker.join().unwrap();
        drop(guard);
        assert!(!supervisor.shutting_down.load(Ordering::Acquire));
        for message in [false, true] { assert!(supervisor.register_start("retry", &Arc::new(AtomicBool::new(false)), message).unwrap()); }
        supervisor.begin_shutdown().unwrap().commit();
        assert!(supervisor.shutting_down.load(Ordering::Acquire));
    }

    #[test]
    fn pause_during_preparation_cannot_be_overwritten_by_worker_activation() {
        let root = std::env::temp_dir().join(format!("supervisor-start-race-{}", uuid::Uuid::new_v4()));
        let store = MultiAgentStore::open_at(root.join("store.db")).unwrap();
        let project = store.create_project(super::super::CreateProjectInput { name: "race".into(), goal: "pause".into(), root_path: root.display().to_string(), default_branch: "main".into() }).unwrap();
        let supervisor = MultiAgentSupervisor::default();
        let cancelled = Arc::new(AtomicBool::new(false));
        supervisor.projects.lock().unwrap().insert(project.id.clone(), Arc::clone(&cancelled));
        let ready = Arc::new(std::sync::Barrier::new(2)); let resume = Arc::new(std::sync::Barrier::new(2));
        let worker = {
            let supervisor = supervisor.clone(); let store = store.clone(); let id = project.id.clone(); let ready = Arc::clone(&ready); let resume = Arc::clone(&resume);
            std::thread::spawn(move || {
                ready.wait(); // preparation reached the activation boundary
                resume.wait();
                let active = supervisor.activate_project_if_current(&store, &id, &cancelled).unwrap();
                supervisor.projects.lock().unwrap().remove(&id);
                active
            })
        };
        ready.wait();
        let pauser = {
            let supervisor = supervisor.clone(); let store = store.clone(); let id = project.id.clone();
            std::thread::spawn(move || supervisor.pause(&store, &id).unwrap())
        };
        let started = Instant::now();
        while store.project(&project.id).unwrap().status != ProjectStatus::Paused {
            assert!(started.elapsed() < Duration::from_secs(3));
            std::thread::sleep(Duration::from_millis(5));
        }
        resume.wait();
        assert!(!worker.join().unwrap());
        assert!(pauser.join().unwrap().accepted);
        assert_eq!(store.project(&project.id).unwrap().status, ProjectStatus::Paused);
        drop(store); std::fs::remove_dir_all(root).unwrap();
    }


    #[test]
    fn supervisor_pause_and_cancel_report_unverified_stop_without_terminalizing_run() {
        let root = std::env::temp_dir().join(format!("supervisor-stop-{}", uuid::Uuid::new_v4()));
        let store = MultiAgentStore::open_at(root.join("store.db")).unwrap();
        let project = store.create_project(super::super::CreateProjectInput { name: "stop".into(), goal: "verify stop".into(), root_path: root.display().to_string(), default_branch: "main".into() }).unwrap();
        let agent = store.create_agent(super::super::CreateAgentInput { avatar_id: None, project_id: project.id.clone(), name: "worker".into(), runtime_kind: AgentRuntimeKind::Codex, role: AgentRole::Worker, description: String::new(), model: None, permission_profile: PermissionProfile::Default, max_concurrency: 1, persona: Default::default(), persona_custom: String::new() }).unwrap();
        let task = store.create_task(super::super::CreateTaskInput { project_id: project.id.clone(), parent_id: None, stage: 1, title: "pending stop".into(), description: String::new(), acceptance_criteria: "stopped".into(), verification_command: None, assignee_agent_id: Some(agent.id.clone()), reviewer_agent_id: None, workspace_mode: WorkspaceMode::Worktree, priority: 0, required_capabilities: Vec::new(), max_attempts: 1, retry_backoff_seconds: 1 }).unwrap();
        let run = store.claim_task(super::super::ClaimTaskInput { task_id: task.id, agent_id: agent.id, lease_seconds: 60 }).unwrap();
        store.mark_run_stop_unverified(&run.id, "fixture kill denied").unwrap();
        let supervisor = MultiAgentSupervisor::default();
        let paused = supervisor.pause(&store, &project.id).unwrap();
        assert!(!paused.accepted && paused.message.contains("停止未确认"));
        let cancelled = supervisor.cancel(&store, &project.id).unwrap();
        assert!(!cancelled.accepted);
        let snapshot = store.snapshot(&project.id).unwrap();
        let saved = snapshot.runs.iter().find(|item| item.id == run.id).unwrap();
        assert_ne!(saved.status, RunStatus::Cancelled);
        assert_eq!(saved.session_locator.as_ref().unwrap()["stopUnverified"], true);
        assert_eq!(snapshot.project.status, ProjectStatus::Paused);
        drop(store);
        std::fs::remove_dir_all(root).unwrap();
    }


    #[cfg(windows)]
    #[test]
    fn reused_visible_host_receives_each_runs_environment_and_removals() {
        let root = std::env::temp_dir().join(format!("visible-env-{}", uuid::Uuid::new_v4()));
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let mut session = sessions.create_runtime(crate::sessions::NewRuntimeSession { runtime_kind: "codex".into(), cwd: root.display().to_string(), project_id: None }).unwrap();
        sessions.set_runtime_command_host(&session.id, true).unwrap();
        let host = AgentTerminalHost::new(PtyManager::new(), sessions.clone());
        let echo = root.join("echo-env.cjs");
        std::fs::write(&echo, "console.log('ENV_RESULT='+process.env.CREWVELA_AGENT_TOKEN+'|'+(process.env.CREWVELA_TEST_PROVIDER??'<missing>'))").unwrap();
        let mut previous = None;
        for (index, token) in ["first-token", "second-token"].into_iter().enumerate() {
            let environment = vec![("CREWVELA_AGENT_TOKEN".into(), Some(token.into())), ("CREWVELA_TEST_PROVIDER".into(), if index == 0 { Some("old-provider".into()) } else { None })];
            let id = host.ensure_environment_terminal(&session, &root, environment.clone()).unwrap();
            if let Some(old) = previous { assert_ne!(id, old); assert!(!host.pty.is_running(old)); }
            session = sessions.attach_runtime(&session.id, id).unwrap();
            assert_eq!(host.ensure_environment_terminal(&session, &root, environment).unwrap(), id);
            let cursor = host.pty.transcript_cursor(id).unwrap();
            host.pty.write(id, format!("node {}\r", powershell_quote(&echo.display().to_string())).as_bytes()).unwrap();
            let expected = format!("ENV_RESULT={token}|{}", if index == 0 { "old-provider" } else { "<missing>" });
            let started = Instant::now(); let mut capture = RuntimeStreamCapture::default();
            loop {
                let (_, bytes) = host.pty.transcript_since(id, cursor).unwrap();
                capture.append(&bytes);
                if capture.output().contains(&expected) { break; }
                assert!(started.elapsed() < Duration::from_secs(15), "child environment was not refreshed: {}", capture.output());
                std::thread::sleep(Duration::from_millis(50));
            }
            previous = Some(id);
        }
        host.pty.kill(previous.unwrap()).unwrap();
        drop(host);
        drop(sessions);
        // ConPTY's system host can release its working-directory handle after
        // the owned Job is already empty. Keep cleanup bounded and fail on any
        // other filesystem error rather than discarding cleanup failures.
        let cleanup_deadline = Instant::now() + Duration::from_secs(3);
        loop {
            match std::fs::remove_dir_all(&root) {
                Ok(()) => break,
                Err(error) if error.raw_os_error() == Some(32) && Instant::now() < cleanup_deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("cannot clean {}: {error}", root.display()),
            }
        }
    }

    #[test]
    fn native_runtime_reuse_requires_current_environment_and_permission_arguments() {
        let mut command = Command::new("fixture");
        command.args(["--permission", "read-only"]).env("CREWVELA_AGENT_TOKEN", "current");
        let binding = NativeRuntimeBinding { kind: AgentRuntimeKind::Codex, cwd: PathBuf::new(), model: None, pty_id: 0, environment: command.get_envs().map(|(key,value)| (key.to_os_string(), value.map(|value| value.to_os_string()))).collect(), program: command.get_program().to_os_string(), arguments: command.get_args().map(|value| value.to_os_string()).collect() };
        assert!(binding.matches_command(&command));
        command.env("CREWVELA_AGENT_TOKEN", "renewed");
        assert!(!binding.matches_command(&command));
        command.env("CREWVELA_AGENT_TOKEN", "current").env_remove("PROVIDER_TOKEN");
        assert!(!binding.matches_command(&command));
        let mut privileged = Command::new("fixture");
        privileged.args(["--permission", "full-access"]).env("CREWVELA_AGENT_TOKEN", "current");
        assert!(!binding.matches_command(&privileged));
    }

    #[test]
    fn failed_stop_keeps_native_binding_and_running_session() {
        let root = std::env::temp_dir().join(format!("native-stop-failure-{}", uuid::Uuid::new_v4()));
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let session = sessions.create_runtime(crate::sessions::NewRuntimeSession { runtime_kind: "codex".into(), cwd: root.display().to_string(), project_id: None }).unwrap();
        sessions.attach_runtime(&session.id, 123).unwrap();
        let host = AgentTerminalHost::new(PtyManager::new(), sessions.clone());
        host.native_runtimes.lock().unwrap().insert(session.id.clone(), NativeRuntimeBinding { kind: AgentRuntimeKind::Codex, model: None, cwd: root.clone(), pty_id: 123, environment: Vec::new(), program: "fixture".into(), arguments: Vec::new() });
        let error = host.finish_native_stop(&session, Err(anyhow!("fixture process kill denied"))).unwrap_err();
        assert!(error.downcast_ref::<UnverifiedStop>().is_some());
        assert!(host.native_runtimes.lock().unwrap().contains_key(&session.id));
        assert_eq!(sessions.get(&session.id).unwrap().pty_id, Some(123));
        assert_ne!(sessions.get(&session.id).unwrap().status, SessionStatus::Stopped);
        host.finish_native_stop(&session, Ok(())).unwrap();
        assert!(!host.native_runtimes.lock().unwrap().contains_key(&session.id));
        assert_eq!(sessions.get(&session.id).unwrap().status, SessionStatus::Stopped);
        std::fs::remove_dir_all(root).unwrap();
    }


    #[test]
    fn kimi_mcp_guard_preserves_external_edits_and_releases_path_lease() {
        let root = std::env::temp_dir().join(format!("kimi-mcp-conflict-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join(".kimi-code")).unwrap();
        let path = root.join(".kimi-code/mcp.json");
        let original = br#"{"mcpServers":{"user":{"command":"custom"}}}"#;
        std::fs::write(&path, original).unwrap();
        let guard = KimiMcpConfigGuard::install_at(&root, "http://fixture").unwrap();
        assert!(KimiMcpConfigGuard::install_at(&root, "http://second").is_err());
        let changed = br#"{"mcpServers":{"outside":{"command":"edited"}}}"#;
        std::fs::write(&path, changed).unwrap();
        drop(guard);
        assert_eq!(std::fs::read(&path).unwrap(), changed);
        let backups = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|entry| entry.unwrap().path()).filter(|path| path.to_string_lossy().contains("crewvela-restore")).collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read(&backups[0]).unwrap(), original);
        drop(KimiMcpConfigGuard::install_at(&root, "http://next").unwrap());
        assert_eq!(std::fs::read(&path).unwrap(), changed);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn readonly_requires_a_runtime_with_enforced_permissions() {
        for runtime in [
            AgentRuntimeKind::KimiCode,
            AgentRuntimeKind::Pi,
            AgentRuntimeKind::OpenCode,
            AgentRuntimeKind::DeepSeekHarness,
            AgentRuntimeKind::ZCode,
            AgentRuntimeKind::ZCodeCli,
        ] {
            assert!(validate_readonly_runtime(WorkspaceMode::ReadOnly, runtime).is_err());
            assert!(validate_readonly_runtime(WorkspaceMode::SharedWrite, runtime).is_ok());
            assert!(validate_readonly_runtime(WorkspaceMode::Worktree, runtime).is_ok());
        }
        assert!(validate_readonly_runtime(WorkspaceMode::ReadOnly, AgentRuntimeKind::ClaudeCode).is_ok());
        assert!(validate_readonly_runtime(WorkspaceMode::ReadOnly, AgentRuntimeKind::Codex).is_ok());
    }

    #[test]
    fn message_workspace_rejects_path_escape_and_project_directory_as_auto_boundary() {
        let config = std::path::Path::new("app-config");
        let project = std::path::Path::new("business-project");
        let task_id = "cli-message-message-42";
        let workspace = message_workspace_at(config, task_id).unwrap();
        assert_eq!(workspace, config.join("message-workspaces").join(task_id));
        assert!(is_isolated_message_workspace(config, task_id, &workspace, project));
        assert!(!is_isolated_message_workspace(config, task_id, project, project));
        assert!(!is_isolated_message_workspace(config, task_id, &workspace, &workspace));
        assert!(!is_isolated_message_workspace(config, task_id, &workspace, config));
        assert!(!is_isolated_message_workspace(config, task_id, &workspace, &workspace.join("business")));
        assert!(!is_isolated_message_workspace(config, task_id, &config.join("arbitrary"), project));
        for invalid in [
            "business-task",
            "cli-message-",
            "cli-message-../outside",
            "cli-message-..\\outside",
            "cli-message-a/../../outside",
            "cli-message-C:outside",
            "cli-message-a\0b",
        ] {
            assert!(message_workspace_at(config, invalid).is_err(), "accepted {invalid:?}");
            assert!(!is_isolated_message_workspace(config, invalid, &workspace, project));
        }
    }

    #[test]
    fn linked_message_workspace_cannot_grant_auto_permissions_to_business_files() {
        let root = std::env::temp_dir().join(format!("crewvela-message-link-{}", uuid::Uuid::new_v4()));
        let config = root.join("config");
        let project = root.join("business");
        std::fs::create_dir_all(config.join("message-workspaces")).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join("sentinel.txt"), "business must remain intact").unwrap();
        let task_id = "cli-message-Linked";
        let workspace = message_workspace_at(&config, task_id).unwrap();
        #[cfg(windows)]
        {
            let output = Command::new("cmd.exe").args(["/D", "/C", "mklink", "/J"])
                .arg(&workspace).arg(&project).output().unwrap();
            assert!(output.status.success(), "junction fixture failed: {}", String::from_utf8_lossy(&output.stderr));
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&project, &workspace).unwrap();
        assert!(!is_isolated_message_workspace(&config, task_id, &workspace, &project));
        assert!(!is_isolated_message_workspace(&config, task_id, &workspace, &root.join("another-business")));
        #[cfg(windows)]
        std::fs::remove_dir(&workspace).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(&workspace).unwrap();
        assert!(project.join("sentinel.txt").exists());
        std::fs::create_dir(&workspace).unwrap();
        assert!(is_isolated_message_workspace(&config, task_id, &workspace, &project));
        std::fs::remove_dir_all(root).unwrap();
    }


    #[test]
    fn zcode_resume_rejects_invalid_identity_and_changed_configuration() {
        let locator = serde_json::json!({"adapter":"zcode_native","protocol":"zcode-v4","sessionId":"session","cwd":"workspace","permissionProfile":"default","model":null});
        let cwd = std::path::Path::new("workspace");
        assert!(validate_zcode_resume(&locator, cwd, PermissionProfile::Default, None).is_ok());
        for (key, value) in [
            ("sessionId", serde_json::Value::Null),
            ("sessionId", serde_json::json!(" ")),
            ("protocol", serde_json::json!("acp")),
            ("cwd", serde_json::json!("other")),
            ("permissionProfile", serde_json::json!("yolo")),
            ("model", serde_json::json!("personal/other")),
        ] {
            let mut changed = locator.clone();
            changed[key] = value;
            assert!(
                validate_zcode_resume(&changed, cwd, PermissionProfile::Default, None).is_err()
            );
        }
    }

    #[test]
    fn zcode_workspace_checkpoint_retries_without_agent_resume() {
        let cwd = std::path::Path::new("workspace");
        // execute_run 的失败与取消路径使用相同的公共检查点格式。
        let checkpoint = serde_json::json!({"cwd": cwd});
        assert_eq!(
            zcode_resume_session(Some(&checkpoint), cwd, PermissionProfile::Default, None).unwrap(),
            None
        );
        let interrupted_start = serde_json::json!({"cwd":cwd,"adapter":"zcode"});
        assert_eq!(
            zcode_resume_session(
                Some(&interrupted_start),
                cwd,
                PermissionProfile::Default,
                None
            )
            .unwrap(),
            None
        );
        let malformed_start =
            serde_json::json!({"cwd":cwd,"adapter":"zcode","sessionId":"session"});
        assert!(zcode_resume_session(
            Some(&malformed_start),
            cwd,
            PermissionProfile::Default,
            None
        )
        .is_err());
        assert!(zcode_resume_session(
            Some(&checkpoint),
            std::path::Path::new("other"),
            PermissionProfile::Default,
            None
        )
        .is_err());
        let incomplete_session = serde_json::json!({"cwd":cwd,"adapter":"zcode_native"});
        assert!(zcode_resume_session(
            Some(&incomplete_session),
            cwd,
            PermissionProfile::Default,
            None
        )
        .is_err());
        let session = serde_json::json!({"cwd":cwd,"adapter":"zcode_native","protocol":"zcode-v4","sessionId":"session","permissionProfile":"default","model":null});
        assert_eq!(
            zcode_resume_session(Some(&session), cwd, PermissionProfile::Default, None).unwrap(),
            Some("session".into())
        );
    }

    #[test]
    fn deepseek_resume_requires_same_workspace_and_permissions() {
        let locator = serde_json::json!({"cwd": "workspace", "permissionMode": "workspace-write", "permissionProfile": "auto"});
        assert!(validate_deepseek_resume(
            &locator,
            std::path::Path::new("workspace"),
            "workspace-write",
            PermissionProfile::Auto
        )
        .is_ok());
        assert!(validate_deepseek_resume(
            &locator,
            std::path::Path::new("workspace"),
            "read-only",
            PermissionProfile::Auto
        )
        .is_err());
        assert!(validate_deepseek_resume(
            &locator,
            std::path::Path::new("workspace"),
            "workspace-write",
            PermissionProfile::Default
        )
        .is_err());
        assert!(validate_deepseek_resume(
            &locator,
            std::path::Path::new("other"),
            "workspace-write",
            PermissionProfile::Auto
        )
        .is_err());
        assert!(validate_deepseek_resume(
            &serde_json::json!({"sessionId": "external"}),
            std::path::Path::new("workspace"),
            "read-only",
            PermissionProfile::Default
        )
        .is_err());
    }

    #[test]
    fn deepseek_permission_details_explain_command_without_secrets() {
        let details = runtime_permission_details(
            &serde_json::json!({"command": "touch example.txt", "path": "example.txt", "api_key": "sensitive", "content": "private file"}),
        );
        assert!(details.contains("touch example.txt"));
        assert!(!details.contains("sensitive"));
        assert!(!details.contains("private file"));
        assert!(!runtime_permission_details(
            &serde_json::json!({"command": "curl -H 'Authorization: Bearer sensitive'"})
        )
        .contains("sensitive"));
    }

    #[test]
    fn native_model_change_requires_a_new_binding_without_reusing_old_model() {
        let binding = NativeRuntimeBinding {
            kind: AgentRuntimeKind::Pi,
            model: Some("old-model".into()),
            cwd: PathBuf::from("project"),
            environment: Vec::new(),
            program: "fixture".into(),
            arguments: Vec::new(),
            pty_id: 3,
        };
        assert!(binding.matches_profile(
            AgentRuntimeKind::Pi,
            std::path::Path::new("project"),
            Some("old-model")
        ));
        assert!(!binding.matches_profile(
            AgentRuntimeKind::Pi,
            std::path::Path::new("project"),
            Some("new-model")
        ));
        assert!(!binding.matches_profile(
            AgentRuntimeKind::Pi,
            std::path::Path::new("project"),
            None
        ));
    }

    #[test]
    fn interactive_prompt_detection_requires_the_idle_prompt() {
        assert!(interactive_runtime_prompt_ready(
            AgentRuntimeKind::KimiCode,
            "\r\n │ > \x1b[7m \x1b[27m │ \r\n K3 thinking: high",
        ));
        assert!(!interactive_runtime_prompt_ready(
            AgentRuntimeKind::KimiCode,
            "Running tool...\r\n",
        ));
        assert!(!interactive_runtime_prompt_ready(
            AgentRuntimeKind::KimiCode,
            "\r\n │ >  │ \r\n Working (3s · esc to cancel)",
        ));
        assert!(!interactive_runtime_prompt_ready(
            AgentRuntimeKind::ClaudeCode,
            "\r> ",
        ));
        assert!(interactive_runtime_prompt_ready(
            AgentRuntimeKind::Codex,
            "OpenAI Codex (v0.154.0)\nmodel: gpt-5.6-sol medium /model to change\n› Ask Codex to do anything",
        ));
        assert!(!interactive_runtime_prompt_ready(
            AgentRuntimeKind::Codex,
            "OpenAI Codex (v0.154.0)\nmodel: gpt-5.6-sol medium /model to change\n› Ask Codex to do anything\nloading...",
        ));
    }

    #[test]
    fn zcode_cli_readiness_uses_the_composer_and_blocks_active_or_login_screens() {
        let ready = |text: &str| interactive_runtime_prompt_ready(AgentRuntimeKind::ZCodeCli, text);
        assert!(ready("\x1b[2J\x1b[HType a prompt\nfixture-model | high"), "narrow mode has no sidebar");
        assert!(ready("\x1b[2J\x1b[HStatus\nRequest timed out\nType a prompt\nfixture-model | high"), "MCP warning is not a busy turn");
        assert!(ready("\x1b[2J\x1b[H输入提示词\nfixture-model | high"));
        for screen in ["ZCODE", "Calling model...\nType to queue input\nesc to interrupt",
            "Help says: Type a prompt\nuser draft",
            "思考中...\n输入内容会排队", "Type a prompt\nPermission required: Allow once",
            "Type a prompt\nNo available models. Configure a provider or sign in with /login.",
            "Type a prompt\nWaiting for browser authorization...", "Type a prompt\nEnter the API key, then press Enter.",
            "Type a prompt\x1b[2J\x1b[Hunknown replacement screen"] {
            assert!(!ready(screen), "must not submit to {screen}");
        }
        assert!(!ready("Type a prompt\nesc to interrupt"));
        assert!(ready("Type to queue input\nesc to interrupt\x1b[2J\x1b[HType a prompt"));
        assert!(ready("Slash commands: /permissions enter to confirm\nType a prompt"), "historical help must not block a fresh empty composer");
    }

    #[test]
    fn opencode_prompt_repaint_recovers_after_busy_and_rejects_permission_choices() {
        let idle = "Ask anything...\nBuild · Space Bunny\ntab agents ctrl+p commands";
        assert!(interactive_runtime_prompt_ready(
            AgentRuntimeKind::OpenCode,
            idle
        ));
        assert!(!interactive_runtime_prompt_ready(
            AgentRuntimeKind::OpenCode,
            &format!("{idle}\nesc interrupt")
        ));
        assert!(interactive_runtime_prompt_ready(
            AgentRuntimeKind::OpenCode,
            &format!("{idle}\nesc interrupt\x1b[2J\x1b[H{idle}")
        ));
        assert!(!interactive_runtime_prompt_ready(
            AgentRuntimeKind::OpenCode,
            &format!("{idle}\nPermission required: Allow once")
        ));
        assert!(!interactive_runtime_prompt_ready(
            AgentRuntimeKind::Pi,
            "> Type a message"
        ));
    }

    #[test]
    fn project_prepares_all_retained_idle_members_as_message_receivers() {
        let root = std::env::temp_dir().join(format!(
            "crewvela-member-message-ready-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = MultiAgentStore::open_at(root.join("project.db")).unwrap();
        let project = store
            .create_project(super::super::CreateProjectInput {
                name: "communication".into(),
                goal: "hello".into(),
                root_path: root.display().to_string(),
                default_branch: "main".into(),
            })
            .unwrap();
        for runtime_kind in [
            AgentRuntimeKind::ClaudeCode,
            AgentRuntimeKind::Codex,
            AgentRuntimeKind::KimiCode,
            AgentRuntimeKind::Pi,
            AgentRuntimeKind::OpenCode,
            AgentRuntimeKind::DeepSeekHarness,
            AgentRuntimeKind::ZCode,
            AgentRuntimeKind::ZCodeCli,
        ] {
            store
                .create_agent(super::super::CreateAgentInput {
                    avatar_id: None,
                    project_id: project.id.clone(),
                    name: runtime_kind.as_str().into(),
                    runtime_kind,
                    role: AgentRole::Worker,
                    description: String::new(),
                    model: None,
                    permission_profile: PermissionProfile::Default,
                    max_concurrency: 1,
                
                persona: Default::default(),
                persona_custom: String::new(),})
                .unwrap();
        }
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let host = AgentTerminalHost::new(PtyManager::new(), sessions.clone());
        host.prepare_project_sessions(&store, &project.id).unwrap();
        let members = sessions.list();
        assert_eq!(members.len(), 8);
        assert!(members
            .iter()
            .all(|member| member.runtime_command_host && member.pty_id.is_none()));
        for member in members {
            assert!(store
                .enqueue_cli_message_task(
                    &project.id,
                    member.multi_agent_agent_id.as_deref().unwrap(),
                    &format!("hello-{}", member.id),
                    "来自其他运行时的 CREWVELA_CLI send --to 消息"
                )
                .unwrap());
            let agent = store
                .agent_profile(member.multi_agent_agent_id.as_deref().unwrap())
                .unwrap();
            let (message_task, _) = store.runnable_tasks(&project.id).unwrap().into_iter()
                .find(|(task, _)| task.assignee_agent_id.as_deref() == Some(agent.id.as_str()))
                .unwrap();
            assert!(message_task.id.starts_with("cli-message-"));
            assert_eq!(message_task.workspace_mode, WorkspaceMode::ReadOnly);
            let mode = execution_workspace_mode(&message_task, agent.runtime_kind);
            assert!(
                validate_readonly_runtime(mode, agent.runtime_kind).is_ok(),
                "{} mailbox turn rejected",
                agent.runtime_kind.as_str()
            );
            assert_eq!(agent.permission_profile, PermissionProfile::Default);
            if matches!(
                agent.runtime_kind,
                AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex
            ) {
                assert_eq!(mode, WorkspaceMode::ReadOnly);
            } else {
                assert_eq!(mode, WorkspaceMode::SharedWrite);
                let mut business_task = message_task.clone();
                business_task.id = "ordinary-business-task".into();
                let business_mode = execution_workspace_mode(&business_task, agent.runtime_kind);
                assert_eq!(business_mode, WorkspaceMode::ReadOnly);
                assert!(validate_readonly_runtime(business_mode, agent.runtime_kind).is_err());
            }
        }
        assert_eq!(store.runnable_tasks(&project.id).unwrap().len(), 8);
        // 历史已移除成员仍在数据库中，不影响可用成员的初始化。
        store.connection.lock().unwrap().execute(
            "UPDATE ma_agents SET runtime_kind='oh_my_pi', enabled=0 WHERE project_id=?1 AND runtime_kind='pi'",
            rusqlite::params![project.id],
        ).unwrap();
        host.prepare_project_sessions(&store, &project.id).unwrap();
        assert_eq!(sessions.list().len(), 8);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn visible_runtime_script_runs_agent_inside_persistent_terminal() {
        let mut command = Command::new("codex.cmd");
        command
            .current_dir(r"F:\project\demo")
            .env("OPENAI_API_KEY", "token'quoted")
            .args(["exec", "--json", "-"]);
        let prompt = PathBuf::from(r"F:\tmp\prompt file.md");
        let output = PathBuf::from(r"F:\tmp\runtime output.jsonl");
        let exit_sentinel = PathBuf::from(r"F:\tmp\runtime exit.txt");
        let script = powershell_runtime_script(
            AgentRuntimeKind::Codex,
            &command,
            Some(prompt.as_path()),
            output.as_path(),
            exit_sentinel.as_path(),
            "__CREWVELA_RUN_DONE_test__:",
        );

        assert!(script.contains("Set-Location -LiteralPath 'F:\\project\\demo'"));
        assert!(!script.contains("OPENAI_API_KEY"));
        assert!(!script.contains("token''quoted"));
        assert!(script.contains("$OutputEncoding = [System.Text.UTF8Encoding]::new($false)"));
        assert!(script
            .contains("Get-Content -Raw -Encoding UTF8 -LiteralPath 'F:\\tmp\\prompt file.md'"));
        assert!(script.contains("F:\\tmp\\runtime output.jsonl"));
        assert!(script.contains("$crewvelaRawTrimAt = 5242880"));
        assert!(script.contains("$crewvelaRawLineTailChars = 1048576"));
        assert!(script.contains("Write-CrewVelaProtocolEvent"));
        assert!(script.contains("& 'codex.cmd' 'exec' '--json' '-'"));
        assert!(script.contains("__CREWVELA_RUN_DONE_test__:"));
        assert!(script.contains("WriteAllText"));
        assert!(script.contains("runtime exit.txt"));
        assert!(powershell_runtime_script(
            AgentRuntimeKind::ClaudeCode,
            &Command::new("工具.cmd"),
            None,
            output.as_path(),
            exit_sentinel.as_path(),
            "done"
        )
        .contains("工具.cmd"));
    }

    #[test]
    fn visible_runtime_marker_reports_exit_code() {
        assert_eq!(
            marker_exit_code(
                "output\r\n__CREWVELA_RUN_DONE_run__:0\r\nPS>",
                "__CREWVELA_RUN_DONE_run__:"
            ),
            Some(0)
        );
        assert_eq!(
            marker_exit_code(
                "__CREWVELA_RUN_DONE_run__:17\r\n",
                "__CREWVELA_RUN_DONE_run__:"
            ),
            Some(17)
        );
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn visible_runtime_keeps_raw_json_for_the_executor_but_shows_readable_progress() {
        let root =
            std::env::temp_dir().join(format!("crewvela-visible-runtime-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mock_runtime = root.join("mock-runtime.ps1");
        let raw_output = root.join("raw-output.jsonl");
        let exit_sentinel = root.join("runtime.exitcode");
        let wrapper = root.join("wrapper.ps1");
        std::fs::write(
            &mock_runtime,
            r#"Write-Output '{"type":"system","subtype":"init","model":"test-model"}'
Write-Output '{"type":"system","subtype":"thinking_tokens","estimated_tokens":12}'
Write-Output '{"type":"assistant","message":{"content":[{"type":"text","text":"已创建 index.html"}]}}'
Write-Output '{"type":"item.completed","item":{"type":"agent_message","content":[{"type":"output_text","text":"codex-review-passed"}]}}'
Write-Output '{"type":"item.completed","item":{"type":"reasoning","content":[{"type":"output_text","text":"hidden-reasoning"}]}}'
Write-Output '{"type":"message_end","message":{"role":"assistant","content":[{"type":"text","text":"pi-message"}]}}'
Write-Output '{"type":"text","text":"opencode-text"}'
Write-Output '{"type":"text","part":{"text":"opencode-part"}}'
Write-Output '{"type":"result","result":"任务完成"}'
exit 0
"#,
        )
        .unwrap();
        let mut command = Command::new("powershell.exe");
        command
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&mock_runtime);
        let script = powershell_runtime_script(
            AgentRuntimeKind::ClaudeCode,
            &command,
            None,
            &raw_output,
            &exit_sentinel,
            "__CREWVELA_RUN_DONE_test__:",
        );
        let mut encoded = vec![0xEF, 0xBB, 0xBF];
        encoded.extend_from_slice(script.as_bytes());
        std::fs::write(&wrapper, encoded).unwrap();

        let result = Command::new("powershell.exe")
            .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
            .arg(&wrapper)
            .output()
            .unwrap();
        let display = String::from_utf8_lossy(&result.stdout);
        let raw = std::fs::read_to_string(&raw_output).unwrap();

        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(display.contains("Claude Code"));
        assert!(display.contains("index.html"));
        assert!(display.contains("codex-review-passed"));
        assert!(display.contains("pi-message"));
        assert!(display.contains("opencode-text"));
        assert!(display.contains("opencode-part"));
        assert!(!display.contains("hidden-reasoning"));
        assert!(!display.contains("thinking_tokens"));
        assert!(raw.contains("thinking_tokens"));
        assert!(raw.contains(r#""type":"result""#));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn readonly_codex_cli_writable_scope_excludes_business_root() {
        let root = std::env::temp_dir().join("crewvela-cli-permissions");
        let scratch = root.join("isolated-scratch");
        let outbox = root.join("outbox");
        let mut command = Command::new("codex");
        super::configure_codex_cli_sandbox(&mut command, Some(&scratch), &outbox);
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        let value_after = |flag| args.windows(2).find(|pair| pair[0] == flag).unwrap()[1].clone();
        assert_eq!(
            value_after("-C"),
            super::external_command_path(&scratch).display().to_string()
        );
        assert_eq!(value_after("-s"), "workspace-write");
        assert_eq!(
            value_after("--add-dir"),
            super::external_command_path(&outbox).display().to_string()
        );
        assert_eq!(args.iter().filter(|arg| *arg == "--add-dir").count(), 1);
        assert!(args.contains(&"sandbox_workspace_write.writable_roots=[]".into()));
        assert!(args.contains(&"sandbox_workspace_write.exclude_tmpdir_env_var=true".into()));
        assert!(args.contains(&"sandbox_workspace_write.exclude_slash_tmp=true".into()));
        assert!(super::validate_readonly_cli_writable_scope(
            &root.join("business"),
            &root.join("business"),
            &scratch,
            &outbox
        )
        .is_ok());
        assert!(
            super::validate_readonly_cli_writable_scope(&root, &root, &scratch, &outbox).is_err()
        );
        assert!(super::validate_readonly_cli_writable_scope(
            &root.join("business"),
            &root.join("business"),
            &scratch,
            &root
        )
        .is_err());
        let mut writable = Command::new("codex");
        super::configure_codex_cli_sandbox(&mut writable, None, &outbox);
        assert!(!writable.get_args().any(|arg| arg == "-C" || arg == "-s"));
    }

    #[test]
    fn runtime_registry_exposes_retained_cli_adapters() {
        let kinds = [
            AgentRuntimeKind::ClaudeCode,
            AgentRuntimeKind::Codex,
            AgentRuntimeKind::KimiCode,
            AgentRuntimeKind::Pi,
            AgentRuntimeKind::OpenCode,
        ];
        for kind in kinds {
            assert_eq!(adapter_for(kind).kind(), kind);
        }
    }

    #[test]
    fn kimi_code_stream_json_keeps_assistant_output_and_resume_locator() {
        let raw = r#"{"role":"meta","type":"system.version","version":"0.36.1"}
{"role":"assistant","content":"实现完成"}
{"role":"meta","type":"session.resume_hint","session_id":"session-kimi-123","command":"kimi -r session-kimi-123","content":"To resume this session: kimi -r session-kimi-123"}"#;

        assert_eq!(
            normalize_output(AgentRuntimeKind::KimiCode, raw),
            "实现完成"
        );
        assert_eq!(
            extract_session_identifier(raw),
            Some(("session_id".to_string(), "session-kimi-123".to_string()))
        );
    }

    #[test]
    fn claude_collaboration_uses_native_interactive_mode() {
        let mut command = Command::new("claude");
        configure_claude_interactive(&mut command, "auto", false, "{}");
        let arguments = command
            .get_args()
            .map(|value| value.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(!arguments.iter().any(|argument| argument == "--effort"));
        assert!(!arguments.iter().any(|argument| argument == "-p"));
        assert!(!arguments
            .iter()
            .any(|argument| argument == "--output-format"));
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["--permission-mode", "auto"]));
        assert!(arguments
            .windows(2)
            .any(|pair| pair == ["--settings", "{}"]));
    }

    #[test]
    fn claude_agent_mcp_config_merges_user_servers_but_crewvela_wins() {
        let mut user_servers = serde_json::Map::new();
        user_servers.insert(
            "filesystem".to_string(),
            serde_json::json!({ "command": "npx", "args": ["-y", "server-filesystem"] }),
        );
        // 用户误配了同名 crewvela，必须被协作桥覆盖。
        user_servers.insert(
            "crewvela".to_string(),
            serde_json::json!({ "command": "impostor" }),
        );

        let config = claude_agent_mcp_config(&user_servers, "http://127.0.0.1:54318/mcp", "secret");

        assert_eq!(
            config["mcpServers"]["crewvela"]["url"],
            "http://127.0.0.1:54318/mcp"
        );
        assert_eq!(
            config["mcpServers"]["crewvela"]["headers"]["Authorization"],
            "Bearer secret"
        );
        assert_eq!(config["mcpServers"]["filesystem"]["command"], "npx");
        assert_eq!(config["mcpServers"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn nested_protocol_events_are_normalized_to_text() {
        let raw = r#"{"type":"item.completed","item":{"type":"agent_message","content":[{"type":"output_text","text":"实现完成"}]}}
{"type":"turn.completed","usage":{"input_tokens":12}}"#;
        assert_eq!(normalize_output(AgentRuntimeKind::Codex, raw), "实现完成");
        assert_eq!(
            extract_session_identifier("{\"type\":\"thread.started\",\"thread_id\":\"abc-123\"}"),
            Some(("thread_id".to_string(), "abc-123".to_string())),
        );
    }

    #[test]
    fn streamed_protocol_output_is_reduced_to_visible_agent_activity() {
        let claude = r#"{"type":"assistant","message":{"content":[{"type":"text","text":"正在实现前端组件"}]}}"#;
        assert_eq!(
            present_runtime_log(AgentRuntimeKind::ClaudeCode, claude, "stdout").as_deref(),
            Some("正在实现前端组件")
        );
        assert_eq!(
            present_runtime_log(
                AgentRuntimeKind::Codex,
                "Reading additional input from stdin...\n",
                "stderr"
            ),
            None
        );
        assert_eq!(
            present_runtime_log(AgentRuntimeKind::Codex, "编译失败", "stderr").as_deref(),
            Some("错误：编译失败")
        );
        let reasoning =
            r#"{"type":"item.completed","item":{"type":"reasoning","text":"私有推理内容"}}"#;
        assert_eq!(
            present_runtime_log(AgentRuntimeKind::Codex, reasoning, "stdout"),
            None
        );
        assert_eq!(normalize_output(AgentRuntimeKind::Codex, reasoning), "");
    }

    #[test]
    fn runtime_output_tail_stays_bounded() {
        let mut tail = VecDeque::new();
        let bytes = vec![b'x'; STREAM_CAPTURE_TAIL_BYTES + 4_096];
        append_bounded_tail(&mut tail, &bytes);
        assert_eq!(tail.len(), STREAM_CAPTURE_TAIL_BYTES);
        assert!(tail.iter().all(|byte| *byte == b'x'));
    }

    #[test]
    fn exited_runtime_does_not_wait_forever_for_an_inherited_output_pipe() {
        let capture = Arc::new(Mutex::new(RuntimeStreamCapture::default()));
        capture.lock().unwrap().append(
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"审查完成"}}
"#
            .as_bytes(),
        );
        let (completion_sender, completion_receiver) = std::sync::mpsc::channel();

        let started = Instant::now();
        let drained = collect_stream_output(
            RuntimeStreamReader {
                completion: completion_receiver,
                capture,
                active: Arc::new(AtomicBool::new(true)),
                thread: None,
            },
            Duration::from_millis(20),
        )
        .unwrap();

        drop(completion_sender);
        assert!(drained.timed_out);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(drained.output.contains("审查完成"));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn timed_out_windows_pipe_read_is_cancelled_and_reaped() {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::Foundation::HANDLE;
        use windows_sys::Win32::System::Pipes::CreatePipe;

        let mut read_handle: HANDLE = std::ptr::null_mut();
        let mut write_handle: HANDLE = std::ptr::null_mut();
        assert_ne!(
            unsafe { CreatePipe(&mut read_handle, &mut write_handle, std::ptr::null(), 0) },
            0
        );
        let read_pipe = unsafe { std::fs::File::from_raw_handle(read_handle) };
        let mut write_pipe = unsafe { std::fs::File::from_raw_handle(write_handle) };
        let root = std::env::temp_dir().join(format!(
            "crewvela-runtime-pipe-test-{}",
            uuid::Uuid::new_v4()
        ));
        let store = MultiAgentStore::open_at(root.join("control.db")).unwrap();
        let stream_reader = spawn_runtime_stream_reader(
            read_pipe,
            store.clone(),
            "missing-run".to_string(),
            "stdout",
            AgentRuntimeKind::Codex,
        );
        write_pipe
            .write_all(
                r#"{"type":"item.completed","item":{"type":"agent_message","text":"审查完成"}}"#
                    .as_bytes(),
            )
            .unwrap();
        write_pipe.flush().unwrap();
        let capture_deadline = Instant::now() + Duration::from_secs(1);
        while !stream_reader
            .capture
            .lock()
            .unwrap()
            .output()
            .contains("审查完成")
        {
            assert!(Instant::now() < capture_deadline, "输出未进入共享缓存");
            std::thread::sleep(Duration::from_millis(5));
        }

        let started = Instant::now();
        let drained = collect_stream_output(stream_reader, Duration::from_millis(20)).unwrap();

        assert!(drained.timed_out);
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(drained.output.contains("审查完成"));
        drop(write_pipe);
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn runtime_event_reader_discards_an_oversized_line_before_reading_the_next_event() {
        let mut bytes = vec![b'x'; RUNTIME_EVENT_MAX_BYTES + 4_096];
        bytes.extend_from_slice(b"\nnext\n");
        let mut reader = BufReader::new(std::io::Cursor::new(bytes));
        let first = read_bounded_event(&mut reader, |_| Ok(()))
            .unwrap()
            .unwrap();
        let second = read_bounded_event(&mut reader, |_| Ok(()))
            .unwrap()
            .unwrap();
        assert_eq!(first.0.len(), RUNTIME_EVENT_MAX_BYTES);
        assert!(first.1);
        assert_eq!(second, (b"next\n".to_vec(), false));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn git_bash_resolution_covers_cmd_and_mingw64_layouts() {
        let bash = PathBuf::from(r"D:\Git\bin\bash.exe");
        let only_bash_exists = |candidate: &std::path::Path| candidate == bash.as_path();
        assert_eq!(
            git_bash_next_to(
                std::path::Path::new(r"D:\Git\cmd\git.exe"),
                only_bash_exists
            ),
            Some(bash.clone())
        );
        assert_eq!(
            git_bash_next_to(
                std::path::Path::new(r"D:\Git\mingw64\bin\git.exe"),
                only_bash_exists
            ),
            Some(bash.clone())
        );
        assert_eq!(
            git_bash_next_to(std::path::Path::new(r"D:\Git\cmd\git.exe"), |_| false),
            None
        );
    }

    #[test]
    fn a_failed_project_start_leaves_no_supervisor_registration_behind() {
        let root =
            std::env::temp_dir().join(format!("crewvela-supervisor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let store = MultiAgentStore::open_at(root.join("control.db")).unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("session-events"))
                .unwrap();
        let terminal_host = AgentTerminalHost::new(PtyManager::new(), sessions);
        let bridge = CollaborationBridge::start(store.clone()).unwrap();
        let supervisor = MultiAgentSupervisor::default();

        let first = supervisor.start(
            store.clone(),
            "missing-project".to_string(),
            terminal_host.clone(),
            bridge.clone(),
        );
        assert!(first.is_err(), "项目不存在时启动应失败");
        let second = supervisor.start(
            store.clone(),
            "missing-project".to_string(),
            terminal_host,
            bridge,
        );
        assert!(
            second.is_err(),
            "失败启动后的重试应再次失败，而不是报告已在运行"
        );
        assert!(
            !supervisor
                .projects
                .lock()
                .unwrap()
                .contains_key("missing-project"),
            "准备失败的项目不应残留在调度表"
        );
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn subdirectory_worktree_uses_project_cwd_and_posix_verification() {
        let root = std::env::temp_dir().join(format!("crewvela-subdir-{}", uuid::Uuid::new_v4()));
        let repository = root.join("repository");
        let project_root = repository.join("demo").join("three-agent-showcase");
        let worktrees = root.join("worktrees");
        std::fs::create_dir_all(&project_root).unwrap();
        git_ok(&repository, &["init"]);
        git_ok(&repository, &["config", "user.name", "CrewVela Test"]);
        git_ok(&repository, &["config", "user.email", "crewvela-test@local"]);
        std::fs::write(project_root.join("game.js"), "const ready = true;\n").unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "base"]);
        let project = MultiAgentProject {
            id: "project".into(),
            name: "subdir".into(),
            goal: "verify cwd".into(),
            root_path: project_root.display().to_string(),
            default_branch: "main".into(),
            status: super::super::ProjectStatus::Active,
            created_at: 0,
            updated_at: 0,
        };
        let task = MultiAgentTask {
            id: "task".into(),
            project_id: project.id.clone(),
            parent_id: None,
            stage: 0,
            title: "verify".into(),
            description: String::new(),
            acceptance_criteria: String::new(),
            verification_command: Some("test -f game.js && node --check game.js".into()),
            status: super::super::TaskStatus::Running,
            assignee_agent_id: None,
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::Worktree,
            priority: 0,
            required_capabilities: Vec::new(),
            max_attempts: 3,
            retry_backoff_seconds: 5,
            next_retry_at: None,
            created_at: 0,
            updated_at: 0,
        };
        let workspace = prepare_workspace_in(&worktrees, &project, &task, "run", &[]).unwrap();
        assert!(workspace.ends_with(std::path::Path::new("demo").join("three-agent-showcase")));
        assert!(verify_task_acceptance(&task, &workspace, &AtomicBool::new(false), None).unwrap());
        let worktree_root = worktrees.join(&project.id).join(&task.id).join("run");
        let _ = hidden_command("git")
            .arg("-C")
            .arg(&repository)
            .args(["worktree", "remove", "--force"])
            .arg(worktree_root)
            .status();
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn read_only_workspace_contains_dependency_commits_without_touching_project_root() {
        let root =
            std::env::temp_dir().join(format!("crewvela-review-workspace-{}", uuid::Uuid::new_v4()));
        let repository = root.join("repository");
        let worktrees = root.join("worktrees");
        std::fs::create_dir_all(&repository).unwrap();
        git_ok(&repository, &["init"]);
        git_ok(&repository, &["config", "user.name", "CrewVela Test"]);
        git_ok(&repository, &["config", "user.email", "crewvela-test@local"]);
        std::fs::write(repository.join("base.txt"), "base").unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "base"]);
        let base = git_head(&repository).unwrap();

        git_ok(&repository, &["checkout", "-b", "implementation"]);
        std::fs::write(repository.join("data.json"), r#"{"marker":"CLAUDE_READY"}"#).unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "implementation"]);
        let delivery_commit = git_head(&repository).unwrap();
        git_ok(&repository, &["checkout", "--detach", &base]);

        let project = MultiAgentProject {
            id: "project".to_string(),
            name: "review snapshot".to_string(),
            goal: "review dependency delivery".to_string(),
            root_path: repository.display().to_string(),
            default_branch: "main".to_string(),
            status: super::super::ProjectStatus::Active,
            created_at: 0,
            updated_at: 0,
        };
        let task = MultiAgentTask {
            id: "review-task".to_string(),
            project_id: project.id.clone(),
            parent_id: None,
            stage: 1,
            title: "review data".to_string(),
            description: String::new(),
            acceptance_criteria: String::new(),
            verification_command: None,
            status: super::super::TaskStatus::Running,
            assignee_agent_id: None,
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::ReadOnly,
            priority: 0,
            required_capabilities: Vec::new(),
            max_attempts: 3,
            retry_backoff_seconds: 5,
            next_retry_at: None,
            created_at: 0,
            updated_at: 0,
        };
        let dependency = ArtifactRecord {
            id: "delivery".to_string(),
            project_id: project.id.clone(),
            task_id: "implementation-task".to_string(),
            run_id: "implementation-run".to_string(),
            artifact_type: "git_commit".to_string(),
            uri: repository.display().to_string(),
            summary: "Claude delivered data.json".to_string(),
            content_hash: Some(delivery_commit),
            created_at: 1,
        };

        let workspace =
            prepare_workspace_in(&worktrees, &project, &task, "review-run", &[dependency]).unwrap();

        assert!(!workspace_can_resume(&project, &task, &repository));
        assert!(!workspace_can_resume(&project, &task, &workspace));
        assert_ne!(workspace, repository);
        assert_eq!(
            std::fs::read_to_string(workspace.join("data.json")).unwrap(),
            r#"{"marker":"CLAUDE_READY"}"#
        );
        assert!(!repository.join("data.json").exists());

        let _ = hidden_command("git")
            .arg("-C")
            .arg(&repository)
            .args(["worktree", "remove", "--force"])
            .arg(&workspace)
            .status();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn read_only_workspace_rejects_agent_mutations() {
        let root = std::env::temp_dir().join(format!("crewvela-read-only-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        git_ok(&root, &["init"]);
        git_ok(&root, &["config", "user.name", "CrewVela Test"]);
        git_ok(&root, &["config", "user.email", "crewvela-test@local"]);
        std::fs::write(root.join("reviewed.txt"), "before").unwrap();
        git_ok(&root, &["add", "."]);
        git_ok(&root, &["commit", "-m", "base"]);
        let baseline_head = git_head(&root).unwrap();
        let task = MultiAgentTask {
            id: "review-task".to_string(),
            project_id: "project".to_string(),
            parent_id: None,
            stage: 1,
            title: "review data".to_string(),
            description: String::new(),
            acceptance_criteria: String::new(),
            verification_command: None,
            status: super::super::TaskStatus::Running,
            assignee_agent_id: None,
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::ReadOnly,
            priority: 0,
            required_capabilities: Vec::new(),
            max_attempts: 3,
            retry_backoff_seconds: 5,
            next_retry_at: None,
            created_at: 0,
            updated_at: 0,
        };

        assert!(ensure_read_only_workspace_clean(&task, &root, Some(&baseline_head)).is_ok());
        std::fs::write(root.join("reviewed.txt"), "after").unwrap();
        let error =
            ensure_read_only_workspace_clean(&task, &root, Some(&baseline_head)).unwrap_err();

        assert!(error.to_string().contains("read_only"));
        git_ok(&root, &["add", "."]);
        git_ok(&root, &["commit", "-m", "forbidden review mutation"]);
        let committed_error =
            ensure_read_only_workspace_clean(&task, &root, Some(&baseline_head)).unwrap_err();
        assert!(committed_error.to_string().contains("提交了审查工作区变更"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_dependency_integration_removes_the_review_worktree() {
        let root =
            std::env::temp_dir().join(format!("crewvela-review-cleanup-{}", uuid::Uuid::new_v4()));
        let repository = root.join("repository");
        let worktrees = root.join("worktrees");
        std::fs::create_dir_all(&repository).unwrap();
        git_ok(&repository, &["init"]);
        git_ok(&repository, &["config", "user.name", "CrewVela Test"]);
        git_ok(&repository, &["config", "user.email", "crewvela-test@local"]);
        std::fs::write(repository.join("conflict.txt"), "base").unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "base"]);
        let base = git_head(&repository).unwrap();

        git_ok(&repository, &["checkout", "-b", "dependency"]);
        std::fs::write(repository.join("conflict.txt"), "dependency").unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "dependency"]);
        let dependency_commit = git_head(&repository).unwrap();
        git_ok(&repository, &["checkout", "--detach", &base]);
        std::fs::write(repository.join("conflict.txt"), "current").unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "current"]);

        let project = MultiAgentProject {
            id: "project".to_string(),
            name: "review cleanup".to_string(),
            goal: "review dependency delivery".to_string(),
            root_path: repository.display().to_string(),
            default_branch: "main".to_string(),
            status: super::super::ProjectStatus::Active,
            created_at: 0,
            updated_at: 0,
        };
        let task = MultiAgentTask {
            id: "review-task".to_string(),
            project_id: project.id.clone(),
            parent_id: None,
            stage: 1,
            title: "review conflict".to_string(),
            description: String::new(),
            acceptance_criteria: String::new(),
            verification_command: None,
            status: super::super::TaskStatus::Running,
            assignee_agent_id: None,
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::ReadOnly,
            priority: 0,
            required_capabilities: Vec::new(),
            max_attempts: 3,
            retry_backoff_seconds: 5,
            next_retry_at: None,
            created_at: 0,
            updated_at: 0,
        };
        let dependency = ArtifactRecord {
            id: "delivery".to_string(),
            project_id: project.id.clone(),
            task_id: "implementation-task".to_string(),
            run_id: "implementation-run".to_string(),
            artifact_type: "git_commit".to_string(),
            uri: repository.display().to_string(),
            summary: "conflicting delivery".to_string(),
            content_hash: Some(dependency_commit),
            created_at: 1,
        };
        let expected_workspace = worktrees
            .join(&project.id)
            .join(&task.id)
            .join("review-run");

        let error = prepare_workspace_in(&worktrees, &project, &task, "review-run", &[dependency])
            .unwrap_err();

        assert!(error.to_string().contains("冲突"));
        assert!(!expected_workspace.exists());
        let worktree_list = hidden_command("git")
            .arg("-C")
            .arg(&repository)
            .args(["worktree", "list", "--porcelain"])
            .output()
            .unwrap();
        assert!(!String::from_utf8_lossy(&worktree_list.stdout)
            .contains(&expected_workspace.display().to_string()));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn read_only_tasks_override_yolo_runtime_permissions() {
        assert_eq!(
            claude_permission_settings(WorkspaceMode::ReadOnly, PermissionProfile::Yolo),
            ("plan", false)
        );
        assert_eq!(
            codex_permission_settings(WorkspaceMode::ReadOnly, PermissionProfile::Yolo),
            CodexPermissionSettings::ReadOnly
        );
        assert!(!uses_dangerous_runtime_permissions(
            WorkspaceMode::ReadOnly,
            PermissionProfile::Auto
        ));
        assert!(!uses_dangerous_runtime_permissions(
            WorkspaceMode::ReadOnly,
            PermissionProfile::Yolo
        ));
        assert!(uses_dangerous_runtime_permissions(
            WorkspaceMode::Worktree,
            PermissionProfile::Yolo
        ));
    }

    #[test]
    fn codex_default_keeps_sandbox_with_supported_approval_policy() {
        for workspace_mode in [WorkspaceMode::Worktree, WorkspaceMode::SharedWrite] {
            assert_eq!(
                codex_permission_settings(workspace_mode, PermissionProfile::Default),
                CodexPermissionSettings::Sandboxed {
                    approval: "on-request"
                }
            );
        }
    }

    #[test]
    fn codex_auto_avoids_the_broken_windows_workspace_sandbox() {
        let settings =
            codex_permission_settings(WorkspaceMode::SharedWrite, PermissionProfile::Auto);
        #[cfg(target_os = "windows")]
        assert_eq!(
            settings,
            CodexPermissionSettings::Unsandboxed { approval: "never" }
        );
        #[cfg(not(target_os = "windows"))]
        assert_eq!(
            settings,
            CodexPermissionSettings::Sandboxed { approval: "never" }
        );
    }

    #[test]
    fn invalid_resume_workspace_discards_the_old_runtime_session() {
        let project = MultiAgentProject {
            id: "project".to_string(),
            name: "review snapshot".to_string(),
            goal: "review dependency delivery".to_string(),
            root_path: "C:\\project".to_string(),
            default_branch: "main".to_string(),
            status: super::super::ProjectStatus::Active,
            created_at: 0,
            updated_at: 0,
        };
        let task = MultiAgentTask {
            id: "review-task".to_string(),
            project_id: project.id.clone(),
            parent_id: None,
            stage: 1,
            title: "review data".to_string(),
            description: String::new(),
            acceptance_criteria: String::new(),
            verification_command: None,
            status: super::super::TaskStatus::Running,
            assignee_agent_id: None,
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::ReadOnly,
            priority: 0,
            required_capabilities: Vec::new(),
            max_attempts: 3,
            retry_backoff_seconds: 5,
            next_retry_at: None,
            created_at: 0,
            updated_at: 0,
        };
        let locator = serde_json::json!({
            "cwd": project.root_path,
            "sessionId": "old-claude-session",
            "threadId": "old-codex-thread"
        });

        let (cwd, locator) = validated_resume_locator(&project, &task, Some(locator));

        assert!(cwd.is_none());
        assert!(locator.is_none());

        let root = std::env::temp_dir().join(format!("non-git-workspace-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let mut project = project;
        project.root_path = root.display().to_string();
        let mut task = task;
        task.workspace_mode = WorkspaceMode::Worktree;
        assert!(!workspace_can_resume(&project, &task, &root));
        let error = prepare_workspace_in(&root.join("worktrees"), &project, &task, "run", &[]).unwrap_err();
        assert!(error.to_string().contains("不是 Git 仓库"));
        task.workspace_mode = WorkspaceMode::SharedWrite;
        assert!(workspace_can_resume(&project, &task, &root));
        assert_eq!(prepare_workspace_in(&root.join("worktrees"), &project, &task, "run", &[]).unwrap(), root);
        task.workspace_mode = WorkspaceMode::ReadOnly;
        assert!(prepare_workspace_in(&root.join("worktrees"), &project, &task, "run", &[]).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn integration_uses_the_audited_artifact_commit_and_its_full_chain() {
        let root =
            std::env::temp_dir().join(format!("crewvela-integration-{}", uuid::Uuid::new_v4()));
        let repository = root.join("repository");
        let target = root.join("target");
        std::fs::create_dir_all(&repository).unwrap();
        git_ok(&repository, &["init"]);
        git_ok(&repository, &["config", "user.name", "CrewVela Test"]);
        git_ok(&repository, &["config", "user.email", "crewvela-test@local"]);
        std::fs::write(repository.join("base.txt"), "base").unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "base"]);
        let base = git_head(&repository).unwrap();

        git_ok(&repository, &["checkout", "-b", "source"]);
        std::fs::write(repository.join("first.txt"), "first").unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "first"]);
        let first_audited = git_head(&repository).unwrap();
        std::fs::write(repository.join("second.txt"), "second").unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "second"]);
        let audited = git_head(&repository).unwrap();
        std::fs::write(repository.join("unreviewed.txt"), "unreviewed").unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "unreviewed"]);

        let target_text = target.display().to_string();
        git_ok(
            &repository,
            &["worktree", "add", "-b", "target", &target_text, &base],
        );
        let first_artifact = ArtifactRecord {
            id: "artifact-first".to_string(),
            project_id: "project".to_string(),
            task_id: "task".to_string(),
            run_id: "run".to_string(),
            artifact_type: "git_commit".to_string(),
            uri: repository.display().to_string(),
            summary: "已审计交付".to_string(),
            content_hash: Some(first_audited),
            created_at: 1,
        };
        let second_artifact = ArtifactRecord {
            id: "artifact-second".to_string(),
            content_hash: Some(audited),
            created_at: 2,
            ..first_artifact.clone()
        };
        integrate_dependency_commits(&target, &[first_artifact, second_artifact]).unwrap();

        assert_eq!(
            std::fs::read_to_string(target.join("first.txt")).unwrap(),
            "first"
        );
        assert_eq!(
            std::fs::read_to_string(target.join("second.txt")).unwrap(),
            "second"
        );
        assert!(!target.join("unreviewed.txt").exists());
        let _ = hidden_command("git")
            .arg("-C")
            .arg(&repository)
            .args(["worktree", "remove", "--force"])
            .arg(&target)
            .status();
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn integrator_approval_merges_only_the_persisted_delivery_commit() {
        let root =
            std::env::temp_dir().join(format!("crewvela-final-merge-{}", uuid::Uuid::new_v4()));
        let repository = root.join("repository");
        let workspace = root.join("integrator");
        std::fs::create_dir_all(&repository).unwrap();
        git_ok(&repository, &["init"]);
        git_ok(&repository, &["config", "user.name", "CrewVela Test"]);
        git_ok(&repository, &["config", "user.email", "crewvela-test@local"]);
        std::fs::write(repository.join("base.txt"), "base").unwrap();
        git_ok(&repository, &["add", "."]);
        git_ok(&repository, &["commit", "-m", "base"]);
        let base = git_head(&repository).unwrap();
        let workspace_text = workspace.display().to_string();
        git_ok(
            &repository,
            &[
                "worktree",
                "add",
                "-b",
                "integrator",
                &workspace_text,
                &base,
            ],
        );
        std::fs::write(workspace.join("audited.txt"), "audited").unwrap();
        git_ok(&workspace, &["add", "."]);
        git_ok(&workspace, &["commit", "-m", "audited"]);
        let audited = git_head(&workspace).unwrap();
        std::fs::write(workspace.join("unreviewed.txt"), "unreviewed").unwrap();
        git_ok(&workspace, &["add", "."]);
        git_ok(&workspace, &["commit", "-m", "unreviewed"]);

        let store = MultiAgentStore::open_at(root.join("control.db")).unwrap();
        let project = store
            .create_project(super::super::CreateProjectInput {
                name: "final merge".to_string(),
                goal: "merge audited integrator delivery".to_string(),
                root_path: repository.display().to_string(),
                default_branch: "main".to_string(),
            })
            .unwrap();
        let agent = store
            .create_agent(super::super::CreateAgentInput {
                avatar_id: None,
                project_id: project.id.clone(),
                name: "Integrator".to_string(),
                runtime_kind: AgentRuntimeKind::Codex,
                role: super::super::AgentRole::Integrator,
                description: "integrate".to_string(),
                model: None,
                permission_profile: PermissionProfile::Yolo,
                max_concurrency: 1,
            
            persona: Default::default(),
            persona_custom: String::new(),})
            .unwrap();
        let task = store
            .create_task(super::super::CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "integrate".to_string(),
                description: String::new(),
                acceptance_criteria: "merged".to_string(),
                verification_command: None,
                assignee_agent_id: Some(agent.id.clone()),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::Worktree,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 3,
                retry_backoff_seconds: 5,
            })
            .unwrap();
        let run = store
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        store
            .update_run(UpdateRunInput {
                run_id: run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        store
            .finalize_successful_run(
                &run.id,
                &workspace.display().to_string(),
                "audited",
                true,
                Some(&audited),
                None,
            )
            .unwrap();

        assert!(merge_integrator_delivery(&store, &task.id).unwrap());
        assert_eq!(
            std::fs::read_to_string(repository.join("audited.txt")).unwrap(),
            "audited"
        );
        assert!(!repository.join("unreviewed.txt").exists());

        drop(store);
        let _ = hidden_command("git")
            .arg("-C")
            .arg(&repository)
            .args(["worktree", "remove", "--force"])
            .arg(&workspace)
            .status();
        let _ = std::fs::remove_dir_all(root);
    }

    fn git_ok(repository: &std::path::Path, args: &[&str]) {
        let output = hidden_command("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    #[test]
    fn codex_failure_prefers_json_error_over_benign_stdin_notice() {
        let stdout = r#"{"type":"turn.failed","error":{"message":"windows sandbox failed: helper_unknown_error"}}"#;
        let stderr = "Reading additional input from stdin...\n";
        assert_eq!(
            runtime_failure_message(AgentRuntimeKind::Codex, stdout, stderr),
            "windows sandbox failed: helper_unknown_error"
        );
    }

    #[test]
    fn codex_turn_completed_is_an_authoritative_visible_runtime_signal() {
        let completed = "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"done\"}}\n{\"type\":\"turn.completed\"}";
        assert!(codex_protocol_turn_completed(completed));
        assert!(!codex_protocol_turn_completed(
            "{\"type\":\"turn.failed\",\"error\":{\"message\":\"boom\"}}"
        ));
    }

    #[test]
    fn terminal_summary_removes_native_tui_control_sequences() {
        let raw = "\u{1b}[2J\u{1b}[32m任务完成\u{1b}[0m\r\n验证通过";
        assert_eq!(terminal_output_summary(raw), "任务完成\n验证通过");
    }

    #[test]
    fn codex_0167_startup_composer_waits_for_initialization() {
        let loading = ">_ OpenAI Codex (v0.167.0)\nC:\\fixture\n› Ask Codex to do anything\n";
        assert!(!codex_native_prompt_ready(loading));
        assert!(codex_native_prompt_ready(&format!("{loading}\x1b]7501;state=idle:app=codex\x1b\\")));
        assert!(!codex_native_prompt_ready(&format!("{loading}\x1b]7501;state=working:app=codex\x07")));
    }

    #[test]
    fn codex_terminal_state_requires_a_complete_codex_signal() {
        for suffix in ["\x07", "\x1b\\"] {
            for (state, ready) in [("idle", true), ("done", true), ("working", false)] {
                let raw = format!("\x1b]7501;state={state}:app=codex{suffix}");
                assert_eq!(codex_terminal_state(&raw), Some((ready, raw.len())));
            }
        }
        for raw in ["state=done:app=codex", "\x1b]7501;state=done:app=codex", "\x1b]7501;state=done:app=other\x07", "\x1b]7501;state=done:app=codex\x07\x1b]7501;state=unknown:app=codex\x07"] {
            assert_eq!(codex_terminal_state(raw), None);
        }
        let busy = "\x1b]7501;state=done:app=codex\x07\x1b]7501;state=working:app=codex\x07";
        assert_eq!(codex_terminal_state(busy), Some((false, busy.len())));
    }

    #[test]
    fn codex_native_prompt_requires_the_real_composer() {
        assert!(!codex_native_prompt_ready("OpenAI Codex\nLoading..."));
        assert!(!codex_native_prompt_ready("Sign in to continue"));
        assert!(!codex_native_prompt_ready(
            "OpenAI Codex\n› Yes, I trust the contents\nEnter to confirm\n? for shortcuts"
        ));
        assert!(codex_native_prompt_ready(
            "\u{1b}[32m>_ OpenAI Codex\u{1b}[0m\n› Build a feature\n? for shortcuts"
        ));
    }

    #[test]
    fn codex_update_prompt_is_detected_for_one_time_skip() {
        assert!(codex_native_update_prompt(
            "Update available!\n1. Update now\n2. Skip\n3. Skip until next version\nPress enter to continue"
        ));
        assert!(codex_native_update_prompt(
            "Update available · 0.162.1 → 0.163.0\n› 1. Update now\n2. Skip\n3. Skip until next version\nenter continue · esc skip"
        ));
        assert!(!codex_native_update_prompt(
            ">_ OpenAI Codex\n› Improve documentation\n? for shortcuts"
        ));
    }

    #[test]
    fn codex_0162_compact_composer_is_ready_but_update_loading_and_active_turn_are_not() {
        let ready = ">_ OpenAI Codex (v0.162.1)\nC:\\fixture\n› Ask Codex to do anything\n";
        assert!(interactive_runtime_prompt_ready(AgentRuntimeKind::Codex, ready));
        assert!(!interactive_runtime_prompt_ready(AgentRuntimeKind::Codex,
            ">_ OpenAI Codex (v0.162.1)\nloading\n› Ask Codex to do anything"));
        assert!(!interactive_runtime_prompt_ready(AgentRuntimeKind::Codex,
            &format!("{ready}\nUpdate available · 0.162.1 → 0.163.0\n› 1. Update now\n2. Skip\nenter continue · esc skip")));
        assert!(!interactive_runtime_prompt_ready(AgentRuntimeKind::Codex,
            &format!("{ready}\nWorking · esc to interrupt")));
        assert!(interactive_runtime_prompt_blocked(
            "Update available · 0.162.1 → 0.163.0\n› 1. Update now\n2. Skip\nenter continue · esc skip"));
        assert!(!interactive_runtime_prompt_ready(AgentRuntimeKind::Codex,
            "• Working (6s • esc to interrupt) · Running hooks\n› Ask Codex to do anything\nGPT-6.1-Sol medium · C:\\fixture"));
        assert!(interactive_runtime_prompt_ready(AgentRuntimeKind::Codex,
            "• Hook failed\nWorked for 7s · 14:30\n› Ask Codex to do anything\nGPT-6.1-Sol medium · C:\\fixture"));
        assert!(interactive_runtime_prompt_ready(AgentRuntimeKind::Codex,
            "› Ask Codex to do anything\nGPT-6.1-Sol medium · C:\\fixture"));
    }

    #[test]
    fn missing_pty_session_error_is_retryable() {
        let error = anyhow!("读取启动输出失败").context("无此会话: 2");
        assert!(missing_pty_session_error(&error));
        assert!(!missing_pty_session_error(&anyhow!("Codex 登录失败")));
        assert!(codex_native_startup_failure(&anyhow!(
            "Codex 原生会话尚未就绪，请处理提示"
        )));
        assert!(!codex_native_startup_failure(&anyhow!(
            "Codex 执行任务失败"
        )));
    }

    #[test]
    fn codex_completion_fallback_requires_a_second_stable_composer() {
        let started = Instant::now();
        let initial = ">_ OpenAI Codex (v0.146.0)\nmodel: gpt-5.6-sol /model to change\n› 编写测试";
        let completed = format!("{initial}\n任务已完成\n› Improve documentation in @filename");
        let mut fallback = CodexNativeCompletionFallback::default();

        assert!(!fallback.observe(initial, true, started));
        assert!(!fallback.observe(&completed, true, started + Duration::from_secs(1)));
        assert!(!fallback.observe(&completed, false, started + Duration::from_millis(2200)));
        assert!(fallback.observe(&completed, false, started + Duration::from_millis(2600)));
    }

    #[test]
    fn codex_compact_busy_composer_never_completes_a_silent_active_turn() {
        let started = Instant::now();
        let busy = "› Ask Codex to do anything\n• Working (6s • esc to interrupt) · Running hooks\n› Ask Codex to do anything\nGPT-6.1-Sol medium · C:\\fixture";
        let mut fallback = CodexNativeCompletionFallback::default();
        assert!(!codex_native_prompt_ready(busy));
        assert!(!fallback.observe(busy, true, started));
        assert!(!fallback.observe(busy, false, started + Duration::from_secs(2)));
        assert!(!fallback.observe(busy, false, started + Duration::from_secs(60)));

        let completed = format!("{busy}\nWorked for 7s · 14:30\n› Ask Codex to do anything\nGPT-6.1-Sol medium · C:\\fixture");
        assert!(codex_native_prompt_ready(&completed));
        assert!(!fallback.observe(&completed, true, started + Duration::from_secs(61)));
        assert!(fallback.observe(&completed, false, started + Duration::from_secs(63)));

        let next_busy = format!("{completed}\n• Working (1s • esc to interrupt)\n› Ask Codex to do anything");
        assert!(!codex_native_prompt_ready(&next_busy));
        assert!(!fallback.observe(&next_busy, true, started + Duration::from_secs(64)));
        assert!(!fallback.observe(&next_busy, false, started + Duration::from_secs(66)));
    }

    #[test]
    fn successful_test_command_with_zero_tests_is_rejected() {
        assert!(verification_reports_zero_tests(
            "npm test",
            "ℹ tests 0\nℹ pass 0",
            ""
        ));
        assert!(verification_reports_zero_tests(
            "cargo test",
            "running 0 tests\ntest result: ok",
            ""
        ));
        assert!(!verification_reports_zero_tests(
            "node --test tests/sum.test.js",
            "ℹ tests 1\nℹ pass 1",
            ""
        ));
        assert!(!verification_reports_zero_tests(
            "npm run build",
            "tests 0",
            ""
        ));
    }

    #[test]
    fn codex_0145_composer_is_ready_without_the_legacy_shortcuts_hint() {
        assert!(codex_native_prompt_ready(
            ">_ OpenAI Codex (v0.145.0)\n\
             model: gpt-5.6-sol xhigh  /model to change\n\
             directory: C:\\workspace\n\
             MCP startup incomplete (failed: cowart_mcp)\n\
             › Improve documentation in @filename\n\
             gpt-5.6-sol xhigh · C:\\workspace"
        ));
    }

    #[test]
    fn codex_legacy_composer_does_not_require_the_brand_header() {
        assert!(codex_native_prompt_ready(
            "› Build a feature\n? for shortcuts"
        ));
    }

    #[test]
    fn codex_stale_composer_before_loading_is_not_ready() {
        assert!(!codex_native_prompt_ready(
            ">_ OpenAI Codex (v0.145.0)\n\
             model: gpt-5.6-sol xhigh  /model to change\n\
             › Previous prompt\n\
             Loading..."
        ));
    }

    #[test]
    fn native_agent_prompt_is_pasted_before_enter_is_sent_separately() {
        #[derive(Debug, PartialEq, Eq)]
        enum InputEvent {
            Write(Vec<u8>),
            WaitUntilRendered,
        }

        let events = std::cell::RefCell::new(Vec::new());
        submit_native_prompt(
            "检查当前项目",
            |bytes| {
                events.borrow_mut().push(InputEvent::Write(bytes.to_vec()));
                Ok(())
            },
            || {
                events.borrow_mut().push(InputEvent::WaitUntilRendered);
                Ok(())
            },
            || Ok(()),
        )
        .unwrap();
        assert_eq!(
            events.into_inner(),
            vec![
                InputEvent::Write("\x1b[200~检查当前项目\x1b[201~".as_bytes().to_vec()),
                InputEvent::WaitUntilRendered,
                InputEvent::Write(b"\r".to_vec()),
            ]
        );
    }

    #[test]
    fn native_agent_prompt_is_not_submitted_after_cancellation() {
        let writes = std::cell::RefCell::new(Vec::new());
        let error = submit_native_prompt(
            "检查当前项目",
            |bytes| {
                writes.borrow_mut().push(bytes.to_vec());
                Ok(())
            },
            || Ok(()),
            || Err(anyhow!("Agent 任务已取消")),
        )
        .unwrap_err();
        assert_eq!(error.to_string(), "Agent 任务已取消");
        assert_eq!(
            writes.into_inner(),
            vec!["\x1b[200~检查当前项目\x1b[201~".as_bytes().to_vec()]
        );
    }

    #[test]
    fn claude_multiline_paste_render_is_detected_before_submit() {
        assert!(!native_prompt_paste_rendered(
            "创建 hello.txt\n内容为 AUTO_ENTER_OK",
            "Claude Code v2.1.220\nauto mode on"
        ));
        assert!(native_prompt_paste_rendered(
            "创建 hello.txt\n内容为 AUTO_ENTER_OK",
            "\u{1b}[2K❯ [Pasted text #1 +2 lines]\u{1b}[0m"
        ));
        assert!(native_prompt_paste_rendered(
            "检查当前项目",
            "\u{1b}[2K❯ 检查当前项目\u{1b}[0m"
        ));
        assert!(!native_prompt_paste_rendered(
            "检查当前项目",
            "\u{1b}[2K❯ 检查当前\u{1b}[0m"
        ));
    }

    #[test]
    fn codex_nested_item_warning_is_not_a_turn_failure() {
        let stdout = r#"{"type":"item.completed","item":{"type":"error","message":"Skill descriptions were shortened"}}
{"type":"turn.completed"}"#;
        assert_eq!(codex_protocol_failure(stdout), None);
    }

    #[test]
    fn codex_windows_sandbox_failure_is_detected_in_completed_output() {
        let stdout = r#"{"type":"item.completed","item":{"type":"agent_message","text":"node_repl diagnostics: windows sandbox failed: helper_unknown_error: setup refresh had errors"}}
{"type":"turn.completed"}"#;
        assert_eq!(
            codex_known_runtime_failure(stdout).as_deref(),
            Some(
                "node_repl diagnostics: windows sandbox failed: helper_unknown_error: setup refresh had errors"
            )
        );
    }

    #[test]
    fn windows_verbatim_paths_are_normalized_for_external_commands() {
        assert_eq!(
            external_command_path(std::path::Path::new(r"\\?\F:\project\demo")).to_string_lossy(),
            r"F:\project\demo"
        );
        assert_eq!(
            external_command_path(std::path::Path::new(r"\\?\UNC\server\share\demo"))
                .to_string_lossy(),
            r"\\server\share\demo"
        );
    }

    #[test]
    fn session_cli_tasks_keep_the_pty_fallback() {
        let task = MultiAgentTask {
            id: "task".into(),
            project_id: "project".into(),
            parent_id: None,
            stage: 0,
            title: "Agent 问候".into(),
            description: "运行 CREWVELA_CLI sessions，再运行 send --to target".into(),
            acceptance_criteria: "消息送达".into(),
            verification_command: None,
            status: crate::multi_agent::TaskStatus::Ready,
            assignee_agent_id: None,
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::SharedWrite,
            priority: 0,
            required_capabilities: Vec::new(),
            max_attempts: 3,
            retry_backoff_seconds: 5,
            next_retry_at: None,
            created_at: 0,
            updated_at: 0,
        };

        assert!(task_requires_session_cli(&task));
        assert_eq!(
            managed_acp_mode(WorkspaceMode::ReadOnly, PermissionProfile::Yolo),
            ManagedAcpMode::ReadOnly,
        );
        assert_eq!(
            managed_acp_mode(WorkspaceMode::Worktree, PermissionProfile::Auto),
            ManagedAcpMode::Auto,
        );
    }

    #[test]
    fn permanent_run_error_survives_anyhow_downcast() {
        let error: anyhow::Error = PermanentRunError("配置错误".to_string()).into();
        assert!(error.downcast_ref::<PermanentRunError>().is_some());
        let contexted = error.context("外层上下文");
        assert!(contexted.downcast_ref::<PermanentRunError>().is_some());
        let plain = anyhow!("普通错误");
        assert!(plain.downcast_ref::<PermanentRunError>().is_none());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn verification_runs_in_a_windows_verbatim_workspace() {
        let workspace =
            std::env::temp_dir().join(format!("crewvela-verification-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(
            workspace.join("claude-handoff.txt"),
            "CLAUDE_TO_CODEX_20260801\n",
        )
        .unwrap();
        let task = MultiAgentTask {
            id: "task".to_string(),
            project_id: "project".to_string(),
            parent_id: None,
            stage: 0,
            title: "handoff".to_string(),
            description: String::new(),
            acceptance_criteria: String::new(),
            verification_command: Some(
                r#"findstr /C:"CLAUDE_TO_CODEX_20260801" claude-handoff.txt"#.to_string(),
            ),
            status: crate::multi_agent::TaskStatus::Running,
            assignee_agent_id: None,
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::SharedWrite,
            priority: 0,
            required_capabilities: Vec::new(),
            max_attempts: 3,
            retry_backoff_seconds: 5,
            next_retry_at: None,
            created_at: 0,
            updated_at: 0,
        };
        let verbatim_workspace = PathBuf::from(format!(r"\\?\{}", workspace.display()));

        let result =
            verify_task_acceptance(&task, &verbatim_workspace, &AtomicBool::new(false), None);

        let _ = std::fs::remove_dir_all(&workspace);
        assert!(result.is_ok(), "{result:?}");
        assert_eq!(result.unwrap(), true);
    }
}
