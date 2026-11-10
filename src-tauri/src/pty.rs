use std::collections::{HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;

use crate::launcher::{build_env, resolve_claude, CLEAR_VARS};
use crate::providers::ProviderConfig;
use crate::sessions::TerminalShell;
use crate::workspace_trust::WorkspaceTrustDetector;

const TRANSCRIPT_BUFFER_BYTES: usize = 2 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    static FAIL_READER_INIT: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
    static SUPPRESS_READER_EOF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn session_cli_args(
    claude_session_id: &str,
    resume: bool,
    auto_mode: bool,
    yolo: bool,
) -> Vec<String> {
    let mut args = vec![
        if resume {
            "--resume".to_string()
        } else {
            "--session-id".to_string()
        },
        claude_session_id.to_string(),
    ];
    if auto_mode {
        args.push("--permission-mode".to_string());
        args.push("auto".to_string());
    }
    if yolo {
        args.push("--dangerously-skip-permissions".to_string());
    }
    args
}

pub(crate) fn build_settings_json(env: &HashMap<String, String>, hook_command: &str) -> String {
    build_settings_json_with_provider(env, hook_command, &serde_json::Map::new())
}

pub(crate) fn build_settings_json_with_provider(
    env: &HashMap<String, String>,
    hook_command: &str,
    provider_settings: &serde_json::Map<String, serde_json::Value>,
) -> String {
    let mut env_obj = serde_json::Map::new();
    for (key, value) in env {
        env_obj.insert(key.clone(), serde_json::Value::String(value.clone()));
        if key.starts_with("ANTHROPIC_DEFAULT_") && key.ends_with("_MODEL") {
            env_obj.insert(
                format!("{key}_NAME"),
                serde_json::Value::String(value.clone()),
            );
        }
    }

    let hook = || {
        serde_json::json!({
            "hooks": [{
                "type": "command",
                "command": hook_command
            }]
        })
    };
    let mut settings = provider_settings.clone();
    settings.insert("env".into(), serde_json::Value::Object(env_obj));

    let hooks = settings
        .entry("hooks")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    if !hooks.is_object() {
        *hooks = serde_json::Value::Object(serde_json::Map::new());
    }
    let hooks = hooks.as_object_mut().expect("hooks 已归一化为对象");
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PostToolUse",
        "PostToolUseFailure",
        "Stop",
        "Notification",
        "SessionEnd",
    ] {
        let entries = hooks
            .entry(event)
            .or_insert_with(|| serde_json::Value::Array(Vec::new()));
        if !entries.is_array() {
            *entries = serde_json::Value::Array(Vec::new());
        }
        let entries = entries.as_array_mut().expect("hook 事件已归一化为数组");
        let already_present = entries.iter().any(|entry| {
            entry["hooks"].as_array().is_some_and(|commands| {
                commands
                    .iter()
                    .any(|command| command["command"].as_str() == Some(hook_command))
            })
        });
        if !already_present {
            entries.push(hook());
        }
    }

    serde_json::Value::Object(settings).to_string()
}

pub(crate) fn build_interactive_settings_json(
    env: &HashMap<String, String>,
    hook_command: &str,
    status_line_command: &str,
    provider_settings: &serde_json::Map<String, serde_json::Value>,
    mcp_servers: Option<&serde_json::Map<String, serde_json::Value>>,
) -> String {
    let mut settings: serde_json::Value = serde_json::from_str(&build_settings_json_with_provider(
        env,
        hook_command,
        provider_settings,
    ))
    .expect("内部 settings JSON 必须有效");
    let settings = settings.as_object_mut().expect("settings 必须是对象");
    // MCP 管理页的 server 并入 mcpServers；provider settings 已有的同名
    // server 视为更专家的手工配置，不覆盖。
    if let Some(mcp_servers) = mcp_servers.filter(|mcp| !mcp.is_empty()) {
        let slot = settings
            .entry("mcpServers")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        if !slot.is_object() {
            *slot = serde_json::Value::Object(serde_json::Map::new());
        }
        let map = slot.as_object_mut().expect("mcpServers 已归一化为对象");
        for (name, entry) in mcp_servers {
            map.entry(name.clone()).or_insert_with(|| entry.clone());
        }
    }
    settings.entry("statusLine").or_insert_with(|| {
        serde_json::json!({
            "type": "command",
            "command": status_line_command,
            "padding": 0
        })
    });
    serde_json::Value::Object(settings.clone()).to_string()
}

pub struct PtyOpenOptions<'a> {
    pub provider: &'a ProviderConfig,
    pub key: &'a str,
    pub cwd: Option<&'a str>,
    pub yolo: bool,
    pub auto_mode: bool,
    pub app_session_id: &'a str,
    pub claude_session_id: &'a str,
    pub resume: bool,
    pub hook_command: &'a str,
    pub status_line_command: &'a str,
    /// 检测到 workspace trust 对话框时自动确认（YOLO 会话，或 worktree
    /// 隔离会话——worktree 是 app 自己从用户已选 repo checkout 的目录，
    /// 信任级别与用户选定目录等价，不该再卡一次人工确认）。
    pub auto_confirm_trust: bool,
    /// MCP 管理页启用的 server，注入 --settings 的 mcpServers 块。
    pub mcp_servers: Option<&'a serde_json::Map<String, serde_json::Value>>,
}

/// 一次轮询返回的单 session 数据块。UTF-8 跨块保留，非法字节按 lossy 规则替换。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PtyChunk {
    pub app_session_id: String,
    pub data: String,
    pub running: bool,
    pub code: Option<u32>,
}

/// 多 session PTY 管理器。每个 session 一个 claude，由 u32 id 标识。
///
/// 锁架构：外层 `sessions` 是 `Mutex<HashMap<u32, Arc<Mutex<Session>>>>`。
/// 公开方法短锁 map 取 Arc，Session 锁只保护句柄和元数据；阻塞写入前须
/// clone writer 并释放 Session 锁，使停止和其他 session 的轮询仍能继续。
#[derive(Clone)]
pub struct PtyManager {
    sessions: Arc<Mutex<HashMap<u32, Arc<Mutex<Session>>>>>,
    next_id: Arc<AtomicU32>,
    admission: crate::runtime_admission::RuntimeAdmission,
}

pub(crate) struct StoppedPtyOutput {
    pub(crate) data: String,
    pub(crate) warning: Option<String>,
}

struct Session {
    app_session_id: String,
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    _process_identity: Option<crate::acp::ProcessIdentityGuard>,
    #[cfg(windows)]
    _windows_job: Option<Arc<crate::acp::ProcessJob>>,
    output: Arc<Mutex<Vec<u8>>>,
    output_decoder: Utf8OutputDecoder,
    reader_finished: Arc<AtomicBool>,
    transcript: Arc<Mutex<TranscriptBuffer>>,
    exit_code: Arc<Mutex<Option<u32>>>,
    awaiting_workspace_trust: Arc<AtomicBool>,
    workspace_trust_resolved: Arc<AtomicBool>,
}

impl Session {
    fn process_exit_code(&self) -> Option<u32> {
        let cached = *self.exit_code.lock().unwrap();
        if cached.is_some() {
            return cached;
        }
        // ConPTY 的输出管道可能在进程退出后仍保持打开，不能等待 reader EOF。
        // try_lock 避免与 reader 的 wait 或停止流程互相阻塞。
        if let Ok(mut child) = self.child.try_lock() {
            if let Ok(Some(status)) = observe_child_exit(child.as_mut()) {
                let code = status.exit_code();
                *self.exit_code.lock().unwrap() = Some(code);
                return Some(code);
            }
        }
        None
    }
}

#[derive(Default)]
struct Utf8OutputDecoder {
    pending: Vec<u8>,
}

impl Utf8OutputDecoder {
    fn decode(&mut self, bytes: &[u8], finished: bool) -> String {
        self.pending.extend_from_slice(bytes);
        let mut decoded = String::new();
        let mut consumed = 0;
        while consumed < self.pending.len() {
            match std::str::from_utf8(&self.pending[consumed..]) {
                Ok(valid) => {
                    decoded.push_str(valid);
                    consumed = self.pending.len();
                }
                Err(error) => {
                    let valid_end = consumed + error.valid_up_to();
                    decoded.push_str(std::str::from_utf8(&self.pending[consumed..valid_end]).expect("UTF-8 valid prefix"));
                    consumed = valid_end;
                    if let Some(invalid) = error.error_len() {
                        decoded.push('\u{fffd}');
                        consumed += invalid;
                    } else {
                        // A partial code point is not invalid until the reader
                        // reaches EOF; the next read/poll may complete it.
                        if finished {
                            decoded.push_str(&String::from_utf8_lossy(&self.pending[consumed..]));
                            consumed = self.pending.len();
                        }
                        break;
                    }
                }
            }
        }
        self.pending.drain(..consumed);
        decoded
    }
}

#[derive(Default)]
struct TranscriptBuffer {
    start_offset: u64,
    bytes: VecDeque<u8>,
}

impl TranscriptBuffer {
    fn cursor(&self) -> u64 {
        self.start_offset.saturating_add(self.bytes.len() as u64)
    }

    fn append(&mut self, bytes: &[u8]) {
        self.bytes.extend(bytes.iter().copied());
        let overflow = self.bytes.len().saturating_sub(TRANSCRIPT_BUFFER_BYTES);
        if overflow > 0 {
            self.bytes.drain(..overflow);
            self.start_offset = self.start_offset.saturating_add(overflow as u64);
        }
    }

    fn since(&self, cursor: u64) -> (u64, Vec<u8>) {
        let relative = cursor.saturating_sub(self.start_offset) as usize;
        let offset = relative.min(self.bytes.len());
        (
            self.cursor(),
            self.bytes.iter().skip(offset).copied().collect(),
        )
    }

    fn tail(&self, max_bytes: usize) -> Vec<u8> {
        self.bytes
            .iter()
            .skip(self.bytes.len().saturating_sub(max_bytes))
            .copied()
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceTrustState {
    Awaiting,
    Starting,
    Resolved,
    Missing,
}

pub struct WorkspaceTrustConfirmation {
    pub confirmed_count: usize,
    pub skipped_count: usize,
    pub failed_session_ids: Vec<String>,
}

impl PtyManager {
    #[cfg(test)]
    pub(crate) fn suppress_next_fixture_reader_eof() { SUPPRESS_READER_EOF.with(|value| value.set(true)); }
    pub(crate) fn admit_start(&self) -> Result<crate::runtime_admission::StartGuard> { self.admission.admit() }
    pub fn new() -> Self {
        Self::with_admission(crate::runtime_admission::RuntimeAdmission::default())
    }

    pub(crate) fn with_admission(admission: crate::runtime_admission::RuntimeAdmission) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            next_id: Arc::new(AtomicU32::new(1)),
            admission,
        }
    }

    /// 启动一个 claude session，返回其 id。
    pub fn open(&self, options: PtyOpenOptions<'_>) -> Result<u32> {
        let PtyOpenOptions {
            provider,
            key,
            cwd,
            yolo,
            auto_mode,
            app_session_id,
            claude_session_id,
            resume,
            hook_command,
            status_line_command,
            auto_confirm_trust,
            mcp_servers,
        } = options;
        let claude = resolve_claude()?;
        // 注入会话身份：格子内的 agent 凭 CREWVELA_SESSION_ID 标识发送方、
        // 凭 CREWVELA_CLI 找到本程序，用 `send` 子命令与其他会话互发消息。
        // 放进 env map 后 build_settings_json 会把它带进 --settings（最高优先级），
        // 进程 env 也带一份，双保险。
        let mut env = build_env(provider, key);
        env.insert("CREWVELA_SESSION_ID".into(), app_session_id.to_string());
        if let Ok(executable) = std::env::current_exe() {
            env.insert("CREWVELA_CLI".into(), executable.display().to_string());
        }

        let work_dir = match cwd {
            Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
            _ => dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")),
        };
        let mut cmd = if cfg!(target_os = "windows")
            && claude
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
        {
            let mut c = CommandBuilder::new("cmd.exe");
            c.arg("/c");
            c.arg(&claude);
            c
        } else {
            CommandBuilder::new(&claude)
        };
        cmd.cwd(work_dir);

        for v in CLEAR_VARS {
            cmd.env_remove(*v);
        }
        for (k, v) in &env {
            cmd.env(k, v);
        }
        for arg in session_cli_args(claude_session_id, resume, auto_mode, yolo) {
            cmd.arg(arg);
        }

        // user settings.json 的 env 块优先级高于 spawn env，会把所有 provider 强制成
        // settings 里的模型。用 --settings 传 per-provider env 覆盖（--settings env 优先级最高，
        // 实测能覆盖 user settings env）。这样保留 user 的 hooks/permissions，只覆盖 env。
        // 同时对 ANTHROPIC_DEFAULT_*_MODEL 同步覆盖 _MODEL_NAME（claude 档位 UI 用它显示），
        // 否则 user settings 的 _NAME（如 glm-5.2）会残留，档位看起来还是旧模型。
        let settings_json = build_interactive_settings_json(
            &env,
            hook_command,
            status_line_command,
            &provider.meta.settings,
            mcp_servers,
        );
        cmd.arg("--settings");
        cmd.arg(&settings_json);
        self.spawn_command(cmd, app_session_id, auto_confirm_trust)
    }

    pub fn open_terminal(
        &self,
        shell: TerminalShell,
        cwd: Option<&str>,
        app_session_id: &str,
    ) -> Result<u32> {
        let executable = shell.executable();
        let work_dir = match cwd {
            Some(path) if !path.is_empty() => std::path::PathBuf::from(path),
            _ => dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")),
        };
        let mut cmd = CommandBuilder::new(executable);
        cmd.cwd(work_dir);
        cmd.env("TERM", "xterm-256color");
        // 终端会话同样注入身份，人在 shell 里也能直接用 send 子命令。
        cmd.env("CREWVELA_SESSION_ID", app_session_id);
        if let Ok(executable) = std::env::current_exe() {
            cmd.env("CREWVELA_CLI", executable.display().to_string());
        }
        if shell == TerminalShell::Powershell {
            cmd.arg("-NoLogo");
        } else if matches!(shell, TerminalShell::Zsh | TerminalShell::Bash) {
            cmd.arg("-l");
        }
        self.spawn_command(cmd, app_session_id, false)
    }

    pub fn open_agent_terminal(
        &self,
        cwd: Option<&str>,
        app_session_id: &str,
        environment: Vec<(OsString, Option<OsString>)>,
    ) -> Result<u32> {
        let work_dir = match cwd {
            Some(path) if !path.is_empty() => std::path::PathBuf::from(path),
            _ => dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")),
        };
        let shell = TerminalShell::agent_host_default();
        let mut cmd = CommandBuilder::new(shell.executable());
        cmd.cwd(work_dir);
        cmd.env("TERM", "xterm-256color");
        if shell == TerminalShell::Powershell {
            cmd.args(["-NoLogo", "-NoProfile"]);
        } else {
            // A task's Provider environment is authoritative. Login profiles
            // may override it, and noninteractive bash would read BASH_ENV.
            cmd.args(["--noprofile", "--norc", "-i"]);
        }
        for (key, value) in environment {
            match value {
                Some(value) => cmd.env(key, value),
                None => cmd.env_remove(key),
            }
        }
        if !cfg!(windows) {
            // The generated task script invokes noninteractive bash too. Do
            // not let inherited startup hooks override the task environment.
            cmd.env_remove("BASH_ENV");
            cmd.env_remove("ENV");
        }
        inject_crewvela_identity(&mut cmd, app_session_id);
        self.spawn_command(cmd, app_session_id, false)
    }

    /// 直接在 PTY 中启动 Agent 的原生交互式 CLI，而不是先打开 shell 再执行包装脚本。
    pub fn open_agent_program(
        &self,
        command: &Command,
        app_session_id: &str,
        auto_confirm_workspace_trust: bool,
    ) -> Result<u32> {
        let mut cmd = build_agent_program_command(command);
        inject_crewvela_identity(&mut cmd, app_session_id);
        self.spawn_command(cmd, app_session_id, auto_confirm_workspace_trust)
    }

    fn spawn_command(
        &self,
        mut cmd: CommandBuilder,
        app_session_id: &str,
        auto_confirm_workspace_trust: bool,
    ) -> Result<u32> {
        let _admission = self.admission.admit()?;
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows: 24,
                cols: 80,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("创建 PTY 失败")?;
        #[cfg(windows)]
        let process_job = crate::acp::create_process_job()?;
        #[cfg(windows)] {
            use std::os::windows::io::AsRawHandle;
            cmd.set_windows_job(process_job.as_handle().as_raw_handle() as usize);
        }
        let child = pair.slave.spawn_command(cmd).context("启动终端进程失败")?;
        drop(pair.slave);
        #[cfg(windows)]
        let (process_identity, identity_error) = match child.as_raw_handle()
            .ok_or_else(|| anyhow!("PTY 缺少 spawn 原始 HANDLE"))
            .and_then(|handle| crate::acp::register_process_handle_with_job(unsafe { std::os::windows::io::BorrowedHandle::borrow_raw(handle) }, Arc::clone(&process_job)))
        {
            Ok(identity) => (Some(identity), None),
            Err(error) => (None, Some(anyhow!(crate::acp::UnverifiedStop(format!("PTY Job 身份登记失败，保留会话跟踪：{error:#}"))))),
        };

        #[cfg(unix)]
        let (process_identity, identity_error) = match child.process_id()
            .ok_or_else(|| anyhow!("PTY 缺少 spawn 原始进程组标识"))
            .and_then(crate::acp::register_process_group_identity)
        {
            Ok(identity) => (Some(identity), None),
            Err(error) => (None, Some(anyhow!(crate::acp::UnverifiedStop(format!("PTY 进程组身份登记失败，保留会话跟踪：{error:#}"))))),
        };
        // Register the process before any fallible pipe initialization. On
        // failure retain a minimal tracked session until checked cleanup wins.
        let pipes = (|| -> Result<_> {
            let writer = pair.master.take_writer().context("获取 PTY writer 失败")?;
            #[cfg(test)]
            if let Some(failure) = FAIL_READER_INIT.with(|failure| failure.borrow_mut().take()) {
                failure();
                anyhow::bail!("fixture PTY reader 初始化失败");
            }
            let reader = pair.master.try_clone_reader().context("获取 PTY reader 失败")?;
            Ok((writer, reader))
        })();
        let (writer, reader, initialization_error) = match pipes {
            Ok((writer, reader)) => (writer, reader, None),
            Err(error) => (Box::new(std::io::sink()) as Box<dyn Write + Send>, Box::new(std::io::empty()) as Box<dyn Read + Send>, Some(error)),
        };
        let writer = Arc::new(Mutex::new(writer));
        let child = Arc::new(Mutex::new(child));
        let output: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
        let reader_finished = Arc::new(AtomicBool::new(false));
        let transcript = Arc::new(Mutex::new(TranscriptBuffer::default()));
        let exit_code: Arc<Mutex<Option<u32>>> = Arc::new(Mutex::new(None));
        let awaiting_workspace_trust = Arc::new(AtomicBool::new(false));
        let workspace_trust_resolved = Arc::new(AtomicBool::new(false));

        // 后台线程把 PTY 输出累积到缓冲区，供前端轮询拉取。
        // reader 只持有 output/transcript/exit_code/child/writer 的 Arc clone，
        // 不锁外层 Session Mutex，因此与轮询/输入路径的锁完全隔离。
        let output_clone = Arc::clone(&output);
        let reader_finished_clone = Arc::clone(&reader_finished);
        let transcript_clone = Arc::clone(&transcript);
        let exit_clone = Arc::clone(&exit_code);
        let child_clone = Arc::clone(&child);
        let awaiting_workspace_trust_clone = Arc::clone(&awaiting_workspace_trust);
        let workspace_trust_resolved_clone = Arc::clone(&workspace_trust_resolved);
        let writer_clone = Arc::clone(&writer);
        #[cfg(test)]
        let suppress_reader_eof = SUPPRESS_READER_EOF.with(|value| value.replace(false));
        std::thread::spawn(move || {
            let mut reader = reader;
            let mut buf = [0u8; 8192];
            let mut trust_detector = WorkspaceTrustDetector::default();
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        output_clone.lock().unwrap().extend_from_slice(&buf[..n]);
                        transcript_clone.lock().unwrap().append(&buf[..n]);
                        if trust_detector.push(&buf[..n]) {
                            let _ = handle_detected_workspace_trust(
                                auto_confirm_workspace_trust,
                                &awaiting_workspace_trust_clone,
                                &workspace_trust_resolved_clone,
                                &writer_clone,
                            );
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Err(_) => break,
                }
            }
            #[cfg(not(test))]
            reader_finished_clone.store(true, Ordering::Release);
            #[cfg(test)]
            if !suppress_reader_eof { reader_finished_clone.store(true, Ordering::Release); }
            collect_child_exit(&child_clone, &exit_clone);
        });

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        // 短锁 map：仅插入，立即释放。Session 本身的重锁（output/child 等）发生在
        // 各自的公开方法里，不会与 map 锁嵌套。
        self.sessions.lock().unwrap().insert(
            id,
            Arc::new(Mutex::new(Session {
                app_session_id: app_session_id.to_string(),
                master: pair.master,
                writer,
                child,
                _process_identity: process_identity,
                #[cfg(windows)]
                _windows_job: Some(process_job),
                output,
                output_decoder: Utf8OutputDecoder::default(),
                reader_finished,
                transcript,
                exit_code,
                awaiting_workspace_trust,
                workspace_trust_resolved,
            })),
        );
        if let Some(error) = initialization_error.or(identity_error) {
            if let Err(stop_error) = self.kill(id) {
                return Err(crate::acp::UnverifiedStop(format!("{error:#}；初始化失败后无法确认停止，保留 PTY {id}：{stop_error:#}")).into());
            }
            return Err(error);
        }
        Ok(id)
    }

    /// 短锁 map 取 session Arc，drop map 锁后再做同步 PTY 写 + flush。
    /// flush 期间若 PTY 缓冲写满阻塞，只卡该 session 自己的 writer 锁，
    /// 不再卡全局 map 锁、不冻住其他 session。
    pub fn write(&self, id: u32, bytes: &[u8]) -> Result<()> {
        let session = self.session_arc(id)?;
        let writer = Arc::clone(&session.lock().unwrap().writer);
        let mut w = writer.lock().unwrap();
        w.write_all(bytes)?;
        w.flush()?;
        Ok(())
    }

    /// Approval keys must target this exact process and finish/cancel before
    /// releasing its approval fence; a detached timeout writer could approve
    /// a later prompt after the caller already reported failure.
    pub(crate) fn write_for_app_session_bounded(&self, id: u32, app_id: &str, bytes: &[u8], timeout: std::time::Duration) -> Result<()> {
        anyhow::ensure!(bytes.len() <= 64, "审批输入超过安全长度");
        let session = self.session_arc(id)?;
        let writer = {
            let session = session.lock().unwrap();
            anyhow::ensure!(session.app_session_id == app_id, "PTY 已属于其他会话，审批已过期");
            Arc::clone(&session.writer)
        };
        #[cfg(windows)]
        return bounded_pipe_write(writer, bytes.to_vec(), timeout, || self.kill(id));
        #[cfg(unix)] {
            let fd = session.lock().unwrap().master.as_raw_fd().ok_or_else(|| anyhow!("PTY 不支持有界输入"))?;
            bounded_posix_write(writer, fd, bytes, timeout)
        }
        #[cfg(not(any(windows, unix)))] { let _ = (writer, timeout); Err(anyhow!("当前平台不支持有界审批输入")) }
    }

    pub fn transcript_cursor(&self, id: u32) -> Result<u64> {
        let session = self.session_arc(id)?;
        let s = session.lock().unwrap();
        let cursor = s.transcript.lock().unwrap().cursor();
        Ok(cursor)
    }

    pub fn transcript_since(&self, id: u32, cursor: u64) -> Result<(u64, Vec<u8>)> {
        let session = self.session_arc(id)?;
        let s = session.lock().unwrap();
        let transcript = s.transcript.lock().unwrap();
        Ok(transcript.since(cursor))
    }

    pub fn transcript_tail(&self, id: u32, max_bytes: usize) -> Result<Vec<u8>> {
        let session = self.session_arc(id)?;
        let s = session.lock().unwrap();
        let transcript = s.transcript.lock().unwrap();
        Ok(transcript.tail(max_bytes))
    }

    pub fn workspace_trust_state(&self, app_session_id: &str) -> WorkspaceTrustState {
        let Some(session) = self.session_arc_by_app_id(app_session_id) else {
            return WorkspaceTrustState::Missing;
        };
        let s = session.lock().unwrap();
        if s.exit_code.lock().is_ok_and(|code| code.is_some()) {
            return WorkspaceTrustState::Missing;
        }
        if s.workspace_trust_resolved.load(Ordering::SeqCst) {
            return WorkspaceTrustState::Resolved;
        }
        if s.awaiting_workspace_trust.load(Ordering::SeqCst) {
            WorkspaceTrustState::Awaiting
        } else {
            WorkspaceTrustState::Starting
        }
    }

    pub fn confirm_workspace_trust(
        &self,
        app_session_ids: &HashSet<String>,
    ) -> WorkspaceTrustConfirmation {
        let mut confirmation = WorkspaceTrustConfirmation {
            confirmed_count: 0,
            skipped_count: 0,
            failed_session_ids: Vec::new(),
        };
        for app_session_id in app_session_ids {
            let Some(session) = self.session_arc_by_app_id(app_session_id) else {
                confirmation.skipped_count += 1;
                continue;
            };
            let (awaiting, resolved, writer) = {
                let session = session.lock().unwrap();
                (Arc::clone(&session.awaiting_workspace_trust), Arc::clone(&session.workspace_trust_resolved), Arc::clone(&session.writer))
            };
            match write_workspace_trust_confirmation(&awaiting, &resolved, &writer) {
                Ok(true) => confirmation.confirmed_count += 1,
                Ok(false) => confirmation.skipped_count += 1,
                Err(_) => confirmation.failed_session_ids.push(app_session_id.clone()),
            }
        }
        confirmation
    }

    pub fn resize(&self, id: u32, rows: u16, cols: u16) -> Result<()> {
        let session = self.session_arc(id)?;
        let s = session.lock().unwrap();
        s.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| anyhow!("resize 失败: {e}"))?;
        Ok(())
    }

    fn terminate_session(&self, id: u32, session: &Arc<Mutex<Session>>) -> Result<()> {
        // Keep the session visible until termination is verified. Only the child
        // handle is held during OS calls; polling and other sessions stay usable.
        let child_handle = Arc::clone(&session.lock().unwrap().child);
        #[cfg(unix)]
        let identity_verified = session.lock().unwrap()._process_identity.as_ref().is_some_and(crate::acp::ProcessIdentityGuard::is_verified);
        let mut child = child_handle.lock().map_err(|_| anyhow!("PTY 子进程锁异常"))?;
        #[cfg(windows)]
        let checked_tree = if let Some(handle) = child.as_raw_handle() {
            // Borrow the actual owned Child HANDLE. Reopening process_id could
            // select another process after PID reuse.
            let handle = unsafe { std::os::windows::io::BorrowedHandle::borrow_raw(handle) };
            crate::acp::stop_process_handle_checked(handle)?;
            true
        } else if child.process_id().is_some() {
            return Err(crate::acp::UnverifiedStop(format!("PTY {id} 缺少原始进程 HANDLE，保留跟踪")).into());
        } else { false };
        #[cfg(unix)]
        let checked_tree = if let Some(pid) = child.process_id() {
            if !identity_verified { crate::acp::terminate_process_tree_checked(pid)?; }
            true
        } else { false };
        #[cfg(not(any(windows, unix)))]
        let checked_tree = false;
        if (!cfg!(unix) || !checked_tree) && observe_child_exit(child.as_mut())?.is_none() {
            if !checked_tree {
                if let Err(error) = child.kill() {
                    if observe_child_exit(child.as_mut())?.is_none() {
                        return Err(anyhow!("终止 PTY {id} 失败，保留进程跟踪：{error}"));
                    }
                }
            }
            let started = std::time::Instant::now();
            while observe_child_exit(child.as_mut())?.is_none() {
                if started.elapsed() > std::time::Duration::from_secs(5) {
                    return Err(anyhow!("PTY {id} 尚未确认退出，保留进程跟踪"));
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        }
        #[cfg(unix)]
        if checked_tree {
            // The group is inactive and its leader has remained waitable until
            // now; only the original owned Child may finally reap it.
            child.wait()?;
        }
        drop(child);
        let mut sessions = self.sessions.lock().unwrap();
        if sessions.get(&id).is_some_and(|current| Arc::ptr_eq(current, session)) {
            sessions.remove(&id);
        }
        Ok(())
    }

    pub fn kill(&self, id: u32) -> Result<()> {
        let session = self.sessions.lock().unwrap().get(&id).cloned();
        if let Some(session) = session {
            self.terminate_session(id, &session)?;
        }
        Ok(())
    }

    pub(crate) fn kill_for_app_session(&self, app_session_id: &str) -> Result<()> {
        let snapshots: Vec<_> = self.sessions.lock().unwrap().iter()
            .map(|(id, session)| (*id, Arc::clone(session))).collect();
        for (id, session) in snapshots {
            if session.lock().unwrap().app_session_id == app_session_id {
                self.terminate_session(id, &session)?;
            }
        }
        Ok(())
    }

    pub fn is_running(&self, id: u32) -> bool {
        let Ok(session) = self.session_arc(id) else {
            return false;
        };
        let s = session.lock().unwrap();
        s.process_exit_code().is_none()
    }

    /// 交接必须确认进程已退出，不能用移除 PTY 映射冒充停止成功。
    pub(crate) fn stop_for_handoff(&self, id: u32) -> Result<()> {
        let session = self.session_arc(id)?;
        self.terminate_session(id, &session)
    }

    /// Natural exit still needs checked tree cleanup, followed by the final
    /// reader drain. Dropping the master closes ConPTY so its reader can reach
    /// EOF; removing the map before capturing these Arcs would lose that tail.
    pub(crate) fn stop_for_exit(&self, id: u32) -> Result<StoppedPtyOutput> {
        self.stop_for_exit_with_timeout(id, std::time::Duration::from_secs(5))
    }

    fn stop_for_exit_with_timeout(&self, id: u32, timeout: std::time::Duration) -> Result<StoppedPtyOutput> {
        let session = self.session_arc(id)?;
        let (output, finished, mut decoder) = {
            let mut state = session.lock().unwrap();
            (Arc::clone(&state.output), Arc::clone(&state.reader_finished), std::mem::take(&mut state.output_decoder))
        };
        if let Err(error) = self.terminate_session(id, &session) {
            session.lock().unwrap().output_decoder = decoder;
            return Err(error);
        }
        drop(session);
        let deadline = std::time::Instant::now() + timeout;
        while !finished.load(Ordering::Acquire) {
            if std::time::Instant::now() >= deadline { break; }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let warning = (!finished.load(Ordering::Acquire)).then(|| format!("终端 {id} 已确认停止，但输出流未在截止时间内结束；尾部输出可能不完整。"));
        let bytes = std::mem::take(&mut *output.lock().unwrap());
        Ok(StoppedPtyOutput { data: decoder.decode(&bytes, true), warning })
    }

    pub fn kill_all(&self) -> Result<()> {
        let sessions: Vec<_> = self.sessions.lock().unwrap().iter()
            .map(|(id, session)| (*id, Arc::clone(session))).collect();
        let errors: Vec<_> = sessions.into_iter()
            .filter_map(|(id, session)| self.terminate_session(id, &session).err())
            .map(|error| error.to_string()).collect();
        if !errors.is_empty() {
            return Err(anyhow!(errors.join("；")));
        }
        Ok(())
    }

    /// 拉取所有活跃 session 的增量。
    /// 先短锁 map clone 所有 Arc 快照，立即释放 map 锁，再逐个取 output/exit_code。
    /// 这样某个 reader 持 output 锁做大块 extend 时，只阻塞该 session 的本次取数，
    /// 不再卡 map 锁、不会让其他 session 的输入/resize 一起冻结。
    pub fn poll_all(&self) -> HashMap<u32, PtyChunk> {
        let snapshots: Vec<(u32, Arc<Mutex<Session>>)> = {
            let guard = self.sessions.lock().unwrap();
            guard.iter().map(|(id, s)| (*id, Arc::clone(s))).collect()
        };
        let mut out = HashMap::new();
        for (id, session) in snapshots {
            let mut s = session.lock().unwrap();
            let finished = s.reader_finished.load(Ordering::Acquire);
            let bytes = std::mem::take(&mut *s.output.lock().unwrap());
            let code = s.process_exit_code();
            let data = s.output_decoder.decode(&bytes, finished);
            out.insert(
                id,
                PtyChunk {
                    app_session_id: s.app_session_id.clone(),
                    data,
                    running: code.is_none(),
                    code,
                },
            );
        }
        out
    }

    /// 短锁 map，按内部 u32 id 取 session Arc。map 锁在 clone Arc 后立即释放。
    fn session_arc(&self, id: u32) -> Result<Arc<Mutex<Session>>> {
        self.sessions
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or_else(|| anyhow!("无此会话: {id}"))
    }

    /// 按 app_session_id 反查。先 clone 全部 Arc 快照（map 锁极短），
    /// 再逐个短锁 session 比对 app_session_id；命中后返回 Arc 供调用方进一步操作。
    fn session_arc_by_app_id(&self, app_session_id: &str) -> Option<Arc<Mutex<Session>>> {
        let snapshots: Vec<Arc<Mutex<Session>>> = {
            let guard = self.sessions.lock().unwrap();
            guard.values().cloned().collect()
        };
        snapshots
            .into_iter()
            .find(|s| s.lock().unwrap().app_session_id == app_session_id)
    }
}

#[cfg(windows)]
fn bounded_pipe_write(
    writer: Arc<Mutex<Box<dyn Write + Send>>>, bytes: Vec<u8>, timeout: std::time::Duration,
    mut stop_stalled: impl FnMut() -> Result<()>,
) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker_cancelled = Arc::clone(&cancelled);
    let deadline = Instant::now() + timeout;
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = (|| -> Result<()> {
            let mut writer = loop {
                if worker_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline { return Err(anyhow!("审批输入截止，未取得输入管道")); }
                match writer.try_lock() {
                    Ok(writer) => break writer,
                    Err(std::sync::TryLockError::Poisoned(_)) => return Err(anyhow!("PTY 输入锁异常")),
                    Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
                }
            };
            if worker_cancelled.load(Ordering::Acquire) || Instant::now() >= deadline { return Err(anyhow!("审批输入已取消")); }
            writer.write_all(&bytes)?;
            writer.flush()?;
            Ok(())
        })();
        let _ = tx.send(result);
    });
    let result = match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(anyhow!("审批输入线程异常")),
        Err(mpsc::RecvTimeoutError::Timeout) => {
            cancelled.store(true, Ordering::Release);
            let cancelling = Instant::now();
            let mut attempted_stop = false;
            loop {
                // The JoinHandle holds this exact thread identity. Retry across
                // the tiny user-mode-before-WriteFile window, never a thread ID.
                unsafe { windows_sys::Win32::System::IO::CancelSynchronousIo(worker.as_raw_handle()); }
                if !attempted_stop && cancelling.elapsed() >= Duration::from_millis(250) {
                    attempted_stop = true;
                    if let Err(error) = stop_stalled() {
                        eprintln!("[crewvela] 审批输入取消未确认，保留本会话写入栅栏：{error:#}");
                    }
                }
                match rx.recv_timeout(Duration::from_millis(10)) {
                    Ok(_) | Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                // Keep this session fenced if the OS cannot cancel/stop it. We
                // must never return while a writer can later send approval keys.
            }
            Err(anyhow!("审批输入写入超时，已取消并确认写入线程结束；结果可能已生效，请核对终端"))
        }
    };
    worker.join().map_err(|_| anyhow!("审批输入线程异常"))?;
    result
}

#[cfg(unix)]
fn bounded_posix_write(writer: Arc<Mutex<Box<dyn Write + Send>>>, fd: std::os::fd::RawFd, bytes: &[u8], timeout: std::time::Duration) -> Result<()> {
    use std::time::{Duration, Instant};
    let deadline = Instant::now() + timeout;
    let _writer = loop {
        if Instant::now() >= deadline { return Err(anyhow!("审批输入截止，未取得输入管道")); }
        match writer.try_lock() {
            Ok(writer) => break writer,
            Err(std::sync::TryLockError::Poisoned(_)) => return Err(anyhow!("PTY 输入锁异常")),
            Err(std::sync::TryLockError::WouldBlock) => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 { return Err(std::io::Error::last_os_error().into()); }
    let result = (|| -> Result<()> {
        let mut offset = 0;
        while offset < bytes.len() {
            if Instant::now() >= deadline { return Err(anyhow!("审批输入截止，结果可能已生效，请核对终端")); }
            let remaining = deadline.saturating_duration_since(Instant::now());
            let mut poll = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
            let ready = unsafe { libc::poll(&mut poll, 1, remaining.as_millis().min(50).max(1) as i32) };
            if ready < 0 {
                let error = std::io::Error::last_os_error();
                if error.kind() == std::io::ErrorKind::Interrupted { continue; }
                return Err(error.into());
            }
            if ready == 0 { continue; }
            if poll.revents & (libc::POLLERR | libc::POLLHUP | libc::POLLNVAL) != 0 { return Err(anyhow!("PTY 输入管道已关闭")); }
            let written = unsafe { libc::write(fd, bytes[offset..].as_ptr().cast(), bytes.len() - offset) };
            if written < 0 {
                let error = std::io::Error::last_os_error();
                if matches!(error.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock) { continue; }
                return Err(error.into());
            }
            if written == 0 { return Err(anyhow!("PTY 输入管道未接受审批字符")); }
            offset += written as usize;
        }
        Ok(())
    })();
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags) } < 0 { return Err(std::io::Error::last_os_error()).map_err(anyhow::Error::from); }
    result
}

fn observe_child_exit(child: &mut (dyn Child + Send + Sync)) -> std::io::Result<Option<portable_pty::ExitStatus>> {
    #[cfg(unix)]
    if let Some(pid) = child.process_id() {
        return crate::acp::peek_process_exit(pid).map(|status| status.map(Into::into));
    }
    child.try_wait()
}

fn collect_child_exit(
    child: &Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    exit: &Arc<Mutex<Option<u32>>>,
) {
    // EOF can precede process exit. A blocking wait while holding child would
    // prevent kill/stop from obtaining that same handle.
    loop {
        let code = match child.lock() {
            Ok(mut child) => match observe_child_exit(child.as_mut()) {
                Ok(Some(status)) => Some(status.exit_code()),
                Ok(None) => None,
                Err(_) => break,
            },
            Err(_) => break,
        };
        if let Some(code) = code {
            *exit.lock().unwrap() = Some(code);
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn handle_detected_workspace_trust(
    auto_confirm: bool,
    awaiting: &AtomicBool,
    resolved: &AtomicBool,
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
) -> Result<bool> {
    awaiting.store(true, Ordering::SeqCst);
    if !auto_confirm {
        return Ok(false);
    }
    write_workspace_trust_confirmation(awaiting, resolved, writer)
}

fn inject_crewvela_identity(command: &mut CommandBuilder, app_session_id: &str) {
    command.env("CREWVELA_SESSION_ID", app_session_id);
    if let Ok(executable) = std::env::current_exe() {
        command.env("CREWVELA_CLI", executable.display().to_string());
    }
}

/// Codex 工具命令会重新构造环境；仅给 Codex 进程设置 env 不足以保证转发。
pub(crate) fn configure_codex_cli_identity(
    command: &mut Command,
    app_session_id: &str,
) -> Result<()> {
    let executable = std::env::current_exe().context("无法确定 CrewVela 可执行文件")?;
    for (key, value) in [
        ("CREWVELA_SESSION_ID", app_session_id.to_string()),
        ("CREWVELA_CLI", executable.display().to_string()),
    ] {
        command.env(key, &value);
        command.arg("-c").arg(format!(
            "shell_environment_policy.set.{key}={}",
            serde_json::to_string(&value)?
        ));
    }

    Ok(())
}
fn build_agent_program_command(command: &Command) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(command.get_program());
    if let Some(cwd) = command.get_current_dir() {
        #[cfg(windows)]
        cmd.cwd(crate::multi_agent::external_command_path(cwd));
        #[cfg(not(windows))]
        cmd.cwd(cwd);
    }
    for argument in command.get_args() {
        cmd.arg(argument);
    }
    for (key, value) in command.get_envs() {
        match value {
            Some(value) => cmd.env(key, value),
            None => cmd.env_remove(key),
        }
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("COLORTERM", "truecolor");
    cmd.env_remove("NO_COLOR");
    cmd
}

fn write_workspace_trust_confirmation(
    awaiting: &AtomicBool,
    resolved: &AtomicBool,
    writer: &Arc<Mutex<Box<dyn Write + Send>>>,
) -> Result<bool> {
    if awaiting
        .compare_exchange(true, false, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Ok(false);
    }
    let result = (|| {
        let mut writer = writer.lock().map_err(|_| anyhow!("PTY writer 锁已损坏"))?;
        writer.write_all(b"1\r")?;
        writer.flush()?;
        Ok(())
    })();
    if let Err(error) = result {
        awaiting.store(true, Ordering::SeqCst);
        return Err(error);
    }
    resolved.store(true, Ordering::SeqCst);
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone)]
    struct SharedWriter(Arc<Mutex<Vec<u8>>>);

    impl Write for SharedWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn utf8_decoder_preserves_every_chunk_boundary_and_lossy_errors() {
        let text = "开始中文🙂\x1b[31m结束\x1b[0m";
        for split in 0..=text.len() {
            let mut decoder = Utf8OutputDecoder::default();
            let first = decoder.decode(&text.as_bytes()[..split], false);
            let second = decoder.decode(&text.as_bytes()[split..], false);
            assert_eq!(format!("{first}{second}"), text, "split at byte {split}");
            assert!(decoder.pending.is_empty());
        }
        let bytes = b"a\xff\xe4\xb8\xadb\xf0\x9f";
        let mut decoder = Utf8OutputDecoder::default();
        let mut decoded = String::new();
        for byte in bytes { decoded.push_str(&decoder.decode(&[*byte], false)); }
        assert!(!decoder.pending.is_empty());
        decoded.push_str(&decoder.decode(&[], true));
        assert_eq!(decoded, String::from_utf8_lossy(bytes));
        assert!(decoder.decode(&[], true).is_empty(), "EOF must flush once");
    }

    #[derive(Debug, Clone)]
    struct FixtureChild {
        killed: Arc<AtomicBool>,
        reject_kill: bool,
        pid: Option<u32>,
        handle: Option<usize>,
        released: Arc<(Mutex<bool>, std::sync::Condvar)>,
    }

    impl portable_pty::ChildKiller for FixtureChild {
        fn kill(&mut self) -> std::io::Result<()> {
            if self.reject_kill { return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, "fixture refuses termination")); }
            self.killed.store(true, Ordering::Release);
            *self.released.0.lock().unwrap() = true;
            self.released.1.notify_all();
            Ok(())
        }
        fn clone_killer(&self) -> Box<dyn portable_pty::ChildKiller + Send + Sync> { Box::new(self.clone()) }
    }

    impl Child for FixtureChild {
        fn try_wait(&mut self) -> std::io::Result<Option<portable_pty::ExitStatus>> {
            Ok(self.killed.load(Ordering::Acquire).then(|| portable_pty::ExitStatus::with_exit_code(137)))
        }
        fn wait(&mut self) -> std::io::Result<portable_pty::ExitStatus> {
            panic!("EOF reaper must not hold the child mutex in a blocking wait")
        }
        fn process_id(&self) -> Option<u32> { self.pid }
        #[cfg(windows)]
        fn as_raw_handle(&self) -> Option<std::os::windows::io::RawHandle> { self.handle.map(|handle| handle as std::os::windows::io::RawHandle) }
    }

    struct BlockingFixtureWriter {
        child: FixtureChild,
        entered: Option<std::sync::mpsc::Sender<()>>,
    }

    impl Write for BlockingFixtureWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if let Some(entered) = self.entered.take() { let _ = entered.send(()); }
            let mut released = self.child.released.0.lock().unwrap();
            while !*released { released = self.child.released.1.wait(released).unwrap(); }
            if self.child.killed.load(Ordering::Acquire) {
                Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "fixture stopped"))
            } else { Ok(bytes.len()) }
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    fn fixture_session(manager: &PtyManager, id: u32, app_id: &str, writer: Box<dyn Write + Send>, child: FixtureChild) {
        let pair = native_pty_system().openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 }).unwrap();
        drop(pair.slave);
        manager.sessions.lock().unwrap().insert(id, Arc::new(Mutex::new(Session {
            app_session_id: app_id.into(), master: pair.master,
            writer: Arc::new(Mutex::new(writer)), child: Arc::new(Mutex::new(Box::new(child))),
            _process_identity: None,
            #[cfg(windows)]
            _windows_job: None,
            output: Arc::new(Mutex::new(b"readable output".to_vec())),
            output_decoder: Utf8OutputDecoder::default(), reader_finished: Arc::new(AtomicBool::new(false)),
            transcript: Arc::new(Mutex::new(TranscriptBuffer::default())), exit_code: Arc::new(Mutex::new(None)),
            awaiting_workspace_trust: Arc::new(AtomicBool::new(true)), workspace_trust_resolved: Arc::new(AtomicBool::new(false)),
        })));
    }

    #[test]
    fn rejected_kill_preserves_tracking_and_kill_all_still_stops_other_sessions() {
        let manager = PtyManager::new();
        let rejected = FixtureChild { handle: None, pid: None, reject_kill: true, killed: Arc::new(AtomicBool::new(false)), released: Arc::new((Mutex::new(false), std::sync::Condvar::new())) };
        fixture_session(&manager, 901, "rejected", Box::new(Vec::<u8>::new()), rejected.clone());
        let error = manager.kill(901).unwrap_err().to_string();
        assert!(error.contains("保留进程跟踪"));
        assert!(manager.is_running(901));
        fixture_session(&manager, 902, "stoppable", Box::new(Vec::<u8>::new()), FixtureChild { handle: None, pid: None, reject_kill: false, killed: Arc::new(AtomicBool::new(false)), released: Arc::new((Mutex::new(false), std::sync::Condvar::new())) });
        assert!(manager.kill_all().is_err());
        assert!(manager.sessions.lock().unwrap().contains_key(&901));
        assert!(!manager.sessions.lock().unwrap().contains_key(&902));
        rejected.killed.store(true, Ordering::Release);
        manager.kill(901).unwrap();
        assert!(!manager.sessions.lock().unwrap().contains_key(&901));
    }

    #[cfg(windows)]
    #[test]
    fn bounded_approval_pipe_write_cancels_full_kernel_pipe_without_late_bytes() {
        use std::os::windows::io::{AsRawHandle, FromRawHandle};
        use std::io::{Read, Write};
        use windows_sys::Win32::System::Pipes::{CreatePipe, GetNamedPipeInfo, PeekNamedPipe};
        let (mut read, mut write) = (std::ptr::null_mut(), std::ptr::null_mut());
        assert_ne!(unsafe { CreatePipe(&mut read, &mut write, std::ptr::null(), 4096) }, 0);
        let mut reader = unsafe { std::fs::File::from_raw_handle(read) };
        let mut writer = unsafe { std::fs::File::from_raw_handle(write) };
        let (mut outbound, mut inbound) = (0, 0);
        assert_ne!(unsafe { GetNamedPipeInfo(writer.as_raw_handle(), std::ptr::null_mut(), &mut outbound, &mut inbound, std::ptr::null_mut()) }, 0);
        let capacity = outbound.max(inbound) as usize;
        assert!((1..=1024*1024).contains(&capacity));
        writer.write_all(&vec![b'x'; capacity]).unwrap();
        let mut available = 0;
        assert_ne!(unsafe { PeekNamedPipe(reader.as_raw_handle(), std::ptr::null_mut(), 0, std::ptr::null_mut(), &mut available, std::ptr::null_mut()) }, 0);
        assert_eq!(available as usize, capacity, "fixture pipe must really be full");
        let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(writer)));
        let started = std::time::Instant::now();
        let error = bounded_pipe_write(Arc::clone(&writer), b"1\r".to_vec(), std::time::Duration::from_millis(100), || Ok(())).unwrap_err();
        assert!(error.to_string().contains("超时"), "{error:#}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        let mut buffered = vec![0; capacity]; reader.read_exact(&mut buffered).unwrap();
        assert!(buffered.iter().all(|byte| *byte == b'x'));
        bounded_pipe_write(writer, b"ok".to_vec(), std::time::Duration::from_secs(1), || Ok(())).unwrap();
        let mut next = [0; 2]; reader.read_exact(&mut next).unwrap();
        assert_eq!(&next, b"ok", "a cancelled approval must never arrive after its fence released");
    }

    #[test]
    fn blocked_input_and_trust_confirmation_do_not_block_poll_or_kill() {
        use std::sync::{mpsc, Condvar};
        use std::time::Duration;
        for trust_confirmation in [false, true] {
            let manager = PtyManager::new();
            let child = FixtureChild { handle: None, pid: None, reject_kill: false, killed: Arc::new(AtomicBool::new(false)), released: Arc::new((Mutex::new(false), Condvar::new())) };
            let (entered_tx, entered_rx) = mpsc::channel();
            fixture_session(&manager, 1, "blocked", Box::new(BlockingFixtureWriter { child: child.clone(), entered: Some(entered_tx) }), child.clone());
            fixture_session(&manager, 2, "healthy", Box::new(Vec::<u8>::new()), FixtureChild { handle: None, pid: None, reject_kill: false, killed: Arc::new(AtomicBool::new(false)), released: Arc::new((Mutex::new(false), Condvar::new())) });
            let writer_manager = manager.clone();
            let writer = std::thread::spawn(move || {
                if trust_confirmation {
                    writer_manager.confirm_workspace_trust(&HashSet::from(["blocked".into()])).failed_session_ids == ["blocked"]
                } else { writer_manager.write(1, b"large input").is_err() }
            });
            entered_rx.recv_timeout(Duration::from_secs(2)).expect("writer must enter its blocking I/O");
            let (done_tx, done_rx) = mpsc::channel();
            let poll_manager = manager.clone();
            let poll = std::thread::spawn(move || {
                let chunks = poll_manager.poll_all();
                let healthy = chunks.get(&2).is_some_and(|chunk| chunk.data == "readable output");
                poll_manager.kill(1).unwrap();
                let _ = done_tx.send(healthy);
            });
            let result = done_rx.recv_timeout(Duration::from_secs(2));
            // Always unblock the fixture before asserting so a failing regression
            // cannot leave detached threads holding up the test executable.
            *child.released.0.lock().unwrap() = true;
            child.released.1.notify_all();
            let writer_failed = writer.join().unwrap();
            poll.join().unwrap();
            manager.kill_all().unwrap();
            assert!(result.unwrap_or(false), "blocked writer froze poll/kill (trust={trust_confirmation})");
            assert!(child.killed.load(Ordering::Acquire));
            assert!(writer_failed, "stopping must release blocked input with an error");
        }
    }

    #[test]
    fn eof_reaper_leaves_child_available_for_stop() {
        use portable_pty::ChildKiller;
        use std::sync::{mpsc, Condvar};
        use std::time::Duration;
        let state = FixtureChild { handle: None, pid: None, reject_kill: false, killed: Arc::new(AtomicBool::new(false)), released: Arc::new((Mutex::new(false), Condvar::new())) };
        let child: Arc<Mutex<Box<dyn Child + Send + Sync>>> = Arc::new(Mutex::new(Box::new(state)));
        let exit = Arc::new(Mutex::new(None));
        let reaper_child = child.clone();
        let reaper_exit = exit.clone();
        let reaper = std::thread::spawn(move || collect_child_exit(&reaper_child, &reaper_exit));
        let (tx, rx) = mpsc::channel();
        let stop_child = child.clone();
        let stop = std::thread::spawn(move || { stop_child.lock().unwrap().kill().unwrap(); let _ = tx.send(()); });
        let stopped = rx.recv_timeout(Duration::from_secs(2)).is_ok();
        stop.join().unwrap();
        reaper.join().unwrap();
        assert!(stopped, "EOF reaper must not monopolize child mutex");
        assert_eq!(*exit.lock().unwrap(), Some(137));
    }

    #[test]
    fn poll_all_keeps_partial_utf8_until_more_bytes_or_reader_eof() {
        use std::sync::Condvar;
        let manager = PtyManager::new();
        let child = FixtureChild { handle: None, pid: None, reject_kill: false, killed: Arc::new(AtomicBool::new(false)), released: Arc::new((Mutex::new(true), Condvar::new())) };
        fixture_session(&manager, 1, "utf8", Box::new(Vec::<u8>::new()), child);
        let session = manager.session_arc(1).unwrap();
        {
            let session = session.lock().unwrap();
            *session.output.lock().unwrap() = vec![0xe4, 0xb8];
        }
        assert_eq!(manager.poll_all().remove(&1).unwrap().data, "");
        {
            let session = session.lock().unwrap();
            session.output.lock().unwrap().push(0xad);
        }
        assert_eq!(manager.poll_all().remove(&1).unwrap().data, "中");
        {
            let session = session.lock().unwrap();
            session.output.lock().unwrap().extend_from_slice(&[0xf0, 0x9f]);
            // Exit status alone must not flush: the reader may still deliver bytes.
            *session.exit_code.lock().unwrap() = Some(0);
        }
        assert_eq!(manager.poll_all().remove(&1).unwrap().data, "");
        {
            let session = session.lock().unwrap();
            session.output.lock().unwrap().extend_from_slice(&[0x99, 0x82, 0xe4]);
            session.reader_finished.store(true, Ordering::Release);
        }
        assert_eq!(manager.poll_all().remove(&1).unwrap().data, "🙂�");
        assert_eq!(manager.poll_all().remove(&1).unwrap().data, "");
        manager.kill_all().unwrap();
    }

    #[test]
    fn next_id_increments() {
        let m = PtyManager::new();
        let a = m.next_id.fetch_add(1, Ordering::SeqCst);
        let b = m.next_id.fetch_add(1, Ordering::SeqCst);
        assert_eq!(a + 1, b);
    }

    #[test]
    fn new_manager_has_empty_sessions() {
        let m = PtyManager::new();
        assert!(m.sessions.lock().unwrap().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn agent_exit_is_reported_while_conpty_master_remains_open() {
        let manager = PtyManager::new();
        let mut command = Command::new("cmd.exe");
        command.args(["/d", "/c", "exit 7"]);
        let id = manager
            .open_agent_program(&command, "exit-regression", false)
            .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while manager.is_running(id) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let chunk = manager.poll_all().remove(&id).unwrap();
        manager.kill(id).unwrap();
        assert!(
            !chunk.running,
            "已退出的 cmd 不应等待 ConPTY 输出 EOF 才更新状态"
        );
        assert_eq!(chunk.code, Some(7));
    }

    #[test]
    fn collaboration_agent_program_receives_cli_identity() {
        let mut command = CommandBuilder::new("agent-cli");
        inject_crewvela_identity(&mut command, "session-a");
        assert_eq!(
            command.get_env("CREWVELA_SESSION_ID"),
            Some(std::ffi::OsStr::new("session-a"))
        );
        assert!(command.get_env("CREWVELA_CLI").is_some());
    }

    #[test]
    fn codex_tool_shell_receives_explicit_cli_identity_without_widening_permissions() {
        let mut command = Command::new("codex");
        command.args(["-a", "on-request", "-s", "workspace-write"]);
        let identity = "session with spaces-\"quoted\"";
        configure_codex_cli_identity(&mut command, identity).unwrap();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(&args[..4], ["-a", "on-request", "-s", "workspace-write"]);
        let overrides = args[4..]
            .chunks_exact(2)
            .map(|pair| {
                assert_eq!(pair[0], "-c");
                let (key, value) = pair[1].split_once('=').unwrap();
                (
                    key.to_string(),
                    serde_json::from_str::<String>(value).unwrap(),
                )
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(overrides.len(), 2);
        assert_eq!(
            overrides["shell_environment_policy.set.CREWVELA_SESSION_ID"],
            identity
        );
        assert_eq!(
            overrides["shell_environment_policy.set.CREWVELA_CLI"],
            std::env::current_exe().unwrap().display().to_string()
        );
        assert!(!args
            .iter()
            .any(|arg| arg.contains("inherit=") || arg.contains("include_only=")));
    }
    #[test]
    fn collaboration_agent_program_enables_truecolor_and_clears_no_color() {
        let mut command = Command::new("agent-cli");
        command.env("NO_COLOR", "1");

        let command = build_agent_program_command(&command);

        assert_eq!(
            command.get_env("TERM"),
            Some(std::ffi::OsStr::new("xterm-256color"))
        );
        assert_eq!(
            command.get_env("COLORTERM"),
            Some(std::ffi::OsStr::new("truecolor"))
        );
        assert_eq!(command.get_env("NO_COLOR"), None);
    }

    #[test]
    #[cfg(windows)]
    fn direct_communication_cmd_keeps_verbatim_project_working_directory() {
        let root = std::env::temp_dir().join(format!("crewvela-cwd-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let canonical = root.canonicalize().unwrap();
        let mut command = Command::new("cmd.exe");
        command.args(["/D", "/C", "echo CREWVELA_CWD=%CD%"]).current_dir(&canonical);
        let ptys = PtyManager::new();
        let id = ptys.open_agent_program(&command, "cwd-fixture", false).unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while ptys.is_running(id) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let output = String::from_utf8_lossy(&ptys.transcript_tail(id, 4096).unwrap()).into_owned();
        ptys.kill(id).unwrap();
        std::fs::remove_dir(&root).unwrap();
        assert!(output.contains(&format!("CREWVELA_CWD={}", root.display())), "CLI changed project directory: {output}");
    }

    #[test]
    fn transcript_buffer_is_bounded_and_supports_incremental_cursors() {
        let mut transcript = TranscriptBuffer::default();
        transcript.append(b"first");
        let cursor = transcript.cursor();
        transcript.append(b"-second");
        let (next, bytes) = transcript.since(cursor);
        assert_eq!(bytes, b"-second");
        assert_eq!(next, transcript.cursor());

        transcript.append(&vec![b'x'; TRANSCRIPT_BUFFER_BYTES + 128]);
        assert_eq!(transcript.bytes.len(), TRANSCRIPT_BUFFER_BYTES);
        assert!(transcript.start_offset >= 128);
    }

    #[test]
    fn workspace_trust_confirmation_only_writes_once_while_awaiting() {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(SharedWriter(Arc::clone(&bytes)))));
        let awaiting = std::sync::atomic::AtomicBool::new(true);
        let resolved = std::sync::atomic::AtomicBool::new(false);

        assert!(write_workspace_trust_confirmation(&awaiting, &resolved, &writer).unwrap());
        assert!(!write_workspace_trust_confirmation(&awaiting, &resolved, &writer).unwrap());
        assert_eq!(*bytes.lock().unwrap(), b"1\r");
        assert!(!awaiting.load(Ordering::SeqCst));
        assert!(resolved.load(Ordering::SeqCst));
    }

    #[test]
    fn workspace_trust_confirmation_skips_active_session() {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(SharedWriter(Arc::clone(&bytes)))));
        let awaiting = AtomicBool::new(false);
        let resolved = AtomicBool::new(false);

        assert!(!write_workspace_trust_confirmation(&awaiting, &resolved, &writer).unwrap());
        assert!(bytes.lock().unwrap().is_empty());
        assert!(!resolved.load(Ordering::SeqCst));
    }

    #[test]
    fn detected_workspace_trust_auto_confirms_only_yolo_sessions() {
        let yolo_bytes = Arc::new(Mutex::new(Vec::new()));
        let yolo_writer: Arc<Mutex<Box<dyn Write + Send>>> =
            Arc::new(Mutex::new(Box::new(SharedWriter(Arc::clone(&yolo_bytes)))));
        let yolo_awaiting = AtomicBool::new(false);
        let yolo_resolved = AtomicBool::new(false);

        assert!(handle_detected_workspace_trust(
            true,
            &yolo_awaiting,
            &yolo_resolved,
            &yolo_writer,
        )
        .unwrap());
        assert_eq!(*yolo_bytes.lock().unwrap(), b"1\r");
        assert!(!yolo_awaiting.load(Ordering::SeqCst));
        assert!(yolo_resolved.load(Ordering::SeqCst));

        let default_bytes = Arc::new(Mutex::new(Vec::new()));
        let default_writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(Box::new(
            SharedWriter(Arc::clone(&default_bytes)),
        )));
        let default_awaiting = AtomicBool::new(false);
        let default_resolved = AtomicBool::new(false);

        assert!(!handle_detected_workspace_trust(
            false,
            &default_awaiting,
            &default_resolved,
            &default_writer,
        )
        .unwrap());
        assert!(default_bytes.lock().unwrap().is_empty());
        assert!(default_awaiting.load(Ordering::SeqCst));
        assert!(!default_resolved.load(Ordering::SeqCst));
    }

    #[test]
    fn claude_session_args_distinguish_new_and_resumed_conversations() {
        let id = "550e8400-e29b-41d4-a716-446655440000";

        assert_eq!(
            session_cli_args(id, false, false, false),
            vec!["--session-id".to_string(), id.to_string()]
        );
        assert_eq!(
            session_cli_args(id, true, false, false),
            vec!["--resume".to_string(), id.to_string()]
        );
    }

    #[test]
    fn auto_mode_adds_the_exact_claude_permission_mode_arguments() {
        let id = "550e8400-e29b-41d4-a716-446655440000";

        assert_eq!(
            session_cli_args(id, false, true, false),
            vec![
                "--session-id".to_string(),
                id.to_string(),
                "--permission-mode".to_string(),
                "auto".to_string(),
            ]
        );
        assert_eq!(
            session_cli_args(id, false, false, false),
            vec!["--session-id".to_string(), id.to_string()]
        );
        assert_eq!(
            session_cli_args(id, false, false, true),
            vec![
                "--session-id".to_string(),
                id.to_string(),
                "--dangerously-skip-permissions".to_string(),
            ]
        );
    }

    #[test]
    fn launch_settings_include_provider_env_and_lifecycle_hooks() {
        let env = HashMap::from([("ANTHROPIC_MODEL".to_string(), "glm-test".to_string())]);
        let settings: serde_json::Value = serde_json::from_str(&build_settings_json(
            &env,
            "crewvela --session-hook events",
        ))
        .unwrap();

        assert_eq!(settings["env"]["ANTHROPIC_MODEL"], "glm-test");
        for event in [
            "SessionStart",
            "UserPromptSubmit",
            "PostToolUse",
            "PostToolUseFailure",
            "Stop",
            "Notification",
            "SessionEnd",
        ] {
            assert_eq!(
                settings["hooks"][event][0]["hooks"][0]["command"],
                "crewvela --session-hook events"
            );
        }
    }

    #[test]
    fn launch_settings_preserve_provider_features_and_override_runtime_env() {
        let env = HashMap::from([("ANTHROPIC_MODEL".to_string(), "runtime-model".to_string())]);
        let provider_settings = serde_json::json!({
            "attribution": { "commit": "", "pr": "" },
            "env": { "ANTHROPIC_MODEL": "stale-model" },
            "permissions": { "allow": ["Read"] }
        })
        .as_object()
        .unwrap()
        .clone();

        let settings: serde_json::Value = serde_json::from_str(&build_settings_json_with_provider(
            &env,
            "crewvela --session-hook events",
            &provider_settings,
        ))
        .unwrap();

        assert_eq!(settings["env"]["ANTHROPIC_MODEL"], "runtime-model");
        assert_eq!(settings["attribution"]["commit"], "");
        assert_eq!(settings["permissions"]["allow"][0], "Read");
        assert_eq!(
            settings["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "crewvela --session-hook events"
        );
    }

    #[test]
    fn interactive_launch_settings_capture_official_statusline_json() {
        let settings: serde_json::Value = serde_json::from_str(&build_interactive_settings_json(
            &HashMap::new(),
            "crewvela --session-hook events",
            "crewvela --session-statusline events app-session",
            &serde_json::Map::new(),
            None,
        ))
        .unwrap();
        assert_eq!(settings["statusLine"]["type"], "command");
        assert_eq!(
            settings["statusLine"]["command"],
            "crewvela --session-statusline events app-session"
        );
    }

    #[test]
    fn interactive_launch_settings_preserve_explicit_provider_statusline() {
        let provider_settings = serde_json::json!({
            "statusLine": { "type": "command", "command": "custom-statusline" }
        })
        .as_object()
        .unwrap()
        .clone();
        let settings: serde_json::Value = serde_json::from_str(&build_interactive_settings_json(
            &HashMap::new(),
            "crewvela --session-hook events",
            "crewvela --session-statusline events app-session",
            &provider_settings,
            None,
        ))
        .unwrap();
        assert_eq!(settings["statusLine"]["command"], "custom-statusline");
    }

    #[test]
    fn interactive_launch_settings_merge_managed_mcp_servers_without_overrides() {
        let mut mcp = serde_json::Map::new();
        mcp.insert(
            "filesystem".to_string(),
            serde_json::json!({ "command": "npx", "args": ["-y", "server-filesystem"] }),
        );
        // provider settings 手工配置的同名 server 不被 MCP 管理页覆盖。
        let provider_settings = serde_json::json!({
            "mcpServers": {
                "filesystem": { "command": "hand-tuned" },
                "builtin": { "command": "keep" }
            }
        })
        .as_object()
        .unwrap()
        .clone();

        let settings: serde_json::Value = serde_json::from_str(&build_interactive_settings_json(
            &HashMap::new(),
            "crewvela --session-hook events",
            "crewvela --session-statusline events app-session",
            &provider_settings,
            Some(&mcp),
        ))
        .unwrap();

        assert_eq!(
            settings["mcpServers"]["filesystem"]["command"],
            "hand-tuned"
        );
        assert_eq!(settings["mcpServers"]["builtin"]["command"], "keep");
    }

    #[test]
    fn interactive_launch_settings_create_mcp_servers_slot_when_absent() {
        let mut mcp = serde_json::Map::new();
        mcp.insert(
            "knowledge-base".to_string(),
            serde_json::json!({ "type": "http", "url": "https://example.com/mcp" }),
        );

        let settings: serde_json::Value = serde_json::from_str(&build_interactive_settings_json(
            &HashMap::new(),
            "crewvela --session-hook events",
            "crewvela --session-statusline events app-session",
            &serde_json::Map::new(),
            Some(&mcp),
        ))
        .unwrap();

        assert_eq!(settings["mcpServers"]["knowledge-base"]["type"], "http");
        assert_eq!(
            settings["mcpServers"]["knowledge-base"]["url"],
            "https://example.com/mcp"
        );
    }

    #[test]
    fn missing_session_helpers_report_absence_without_deadlock() {
        let manager = PtyManager::new();

        // 未注册的 session：write/resize 应返回错误，is_running 返回 false，
        // poll_all 返回空（验证空 map 下 clone 快照 + 释放锁的路径）。
        assert!(manager.write(999, b"x").is_err());
        assert!(manager.resize(999, 24, 80).is_err());
        assert!(!manager.is_running(999));
        assert!(manager.poll_all().is_empty());
        assert_eq!(
            manager.workspace_trust_state("nope"),
            WorkspaceTrustState::Missing
        );
        // kill 不存在的 id 不应 panic / 死锁。
        assert!(manager.kill(999).is_ok());
        assert!(manager.kill_all().is_ok());
    }

    #[test]
    fn poll_all_clone_snapshot_then_release_map_lock() {
        // 结构性验证：poll_all 在空 manager 上能跑通，且不持有 sessions 锁。
        // 这里通过「poll_all 之后能立即再次获取锁」来间接确认它不会泄漏持锁。
        let manager = PtyManager::new();
        let _ = manager.poll_all();
        // 若 poll_all 泄漏了 sessions guard，下一次 lock 会死锁（测试卡住 / panic）。
        let guard = manager.sessions.lock().unwrap();
        assert!(guard.is_empty());
        drop(guard);
    }

    #[test]
    fn transcript_tail_returns_only_requested_recent_bytes() {
        let mut transcript = TranscriptBuffer::default();
        transcript.append(b"context old\ncontext: 42% (420k/1M)");
        assert_eq!(transcript.tail(20), b"ntext: 42% (420k/1M)".to_vec(),);
    }

    #[cfg(windows)]
    #[test]
    fn reader_initialization_failure_stops_spawned_root_and_child() {
        use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
        use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE};
        let root = std::env::temp_dir().join(format!("pty-reader-failure-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let ready = root.join("ready.json");
        let handles = Arc::new(Mutex::new(Vec::<OwnedHandle>::new()));
        let captured = Arc::clone(&handles);
        let wait_ready = ready.clone();
        FAIL_READER_INIT.with(|failure| *failure.borrow_mut() = Some(Box::new(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            let pids: Vec<u32> = loop {
                if let Ok(bytes) = std::fs::read(&wait_ready) {
                    if let Ok(pids) = serde_json::from_slice(&bytes) { break pids; }
                }
                assert!(std::time::Instant::now() < deadline, "fixture must launch both processes before injected failure");
                std::thread::sleep(std::time::Duration::from_millis(10));
            };
            for pid in pids {
                let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) };
                assert!(!handle.is_null());
                captured.lock().unwrap().push(unsafe { OwnedHandle::from_raw_handle(handle) });
            }
        })));
        let mut command = Command::new(which::which("node").unwrap());
        command.args(["-e", "const fs=require('node:fs');const c=require('node:child_process').spawn(process.execPath,['-e',\"process.send('ready');setInterval(()=>{},1000)\"],{stdio:['ignore','ignore','ignore','ipc'],detached:true});c.on('message',()=>{fs.writeFileSync(process.argv[1]+'.tmp',JSON.stringify([process.pid,c.pid]));fs.renameSync(process.argv[1]+'.tmp',process.argv[1]);});setInterval(()=>{},1000);"]).arg(&ready);
        let manager = PtyManager::new();
        let error = manager.open_agent_program(&command, "reader-init-failure", false).err().unwrap();
        assert!(error.to_string().contains("reader 初始化失败"), "{error:#}");
        assert!(manager.sessions.lock().unwrap().is_empty());
        let captured = handles.lock().unwrap();
        assert_eq!(captured.len(), 2);
        for handle in captured.iter() { assert_eq!(unsafe { WaitForSingleObject(handle.as_raw_handle(), 1000) }, windows_sys::Win32::Foundation::WAIT_OBJECT_0); }
        drop(captured); drop(handles);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn posix_agent_host_preserves_provider_environment_against_shell_profiles() {
        let root = std::env::temp_dir().join(format!("agent-profile-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        for profile in [".bashrc", ".bash_profile", ".profile", ".zshrc", ".zprofile", "malicious-env"] {
            std::fs::write(root.join(profile), "export ANTHROPIC_BASE_URL=profile-overrode-provider\n").unwrap();
        }
        let environment = vec![
            (OsString::from("HOME"), Some(root.as_os_str().to_os_string())),
            (OsString::from("ZDOTDIR"), Some(root.as_os_str().to_os_string())),
            (OsString::from("BASH_ENV"), Some(root.join("malicious-env").into_os_string())),
            (OsString::from("ENV"), Some(root.join("malicious-env").into_os_string())),
            (OsString::from("ANTHROPIC_BASE_URL"), Some(OsString::from("selected-provider"))),
        ];
        let manager = PtyManager::new();
        let id = manager.open_agent_terminal(None, "provider-profile-fixture", environment).unwrap();
        manager.write(id, b"/bin/bash -c 'printf \"\\nROUTE=%s\\nHOOKS=%s/%s\\n\" \"$ANTHROPIC_BASE_URL\" \"${BASH_ENV-unset}\" \"${ENV-unset}\"'\r").unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut output = String::new();
        loop {
            if let Some(chunk) = manager.poll_all().remove(&id) { output.push_str(&chunk.data); }
            if output.lines().any(|line| line.trim() == "HOOKS=unset/unset") || std::time::Instant::now() >= deadline { break; }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        manager.stop_for_handoff(id).unwrap();
        std::fs::remove_dir_all(root).unwrap();
        assert!(output.lines().any(|line| line.trim() == "ROUTE=selected-provider"), "{output}");
        assert!(!output.contains("ROUTE=profile-overrode-provider"), "{output}");
        assert!(output.lines().any(|line| line.trim() == "HOOKS=unset/unset"), "{output}");
    }

    #[cfg(unix)]
    #[test]
    fn posix_agent_shell_stop_reaps_owned_task_group() {
        let manager = PtyManager::new();
        let id = manager.open_agent_terminal(None, "posix-shell-stop", Vec::new()).unwrap();
        // The visible-task launch disables shell job control so its pipeline
        // remains in the spawn-owned process group. HUP alone cannot stop this fixture.
        manager.write(id, b"set +m; /bin/bash -c 'trap \"\" HUP; echo OWNED_TASK_PID=$$; sleep 120'\r").unwrap();
        let started = std::time::Instant::now();
        let mut output = String::new();
        let pid = loop {
            if let Some(chunk) = manager.poll_all().remove(&id) { output.push_str(&chunk.data); }
            if let Some(pid) = output.split("OWNED_TASK_PID=").filter_map(|tail| tail.split(|c: char| !c.is_ascii_digit()).next()?.parse::<u32>().ok()).next() { break pid; }
            assert!(started.elapsed() < std::time::Duration::from_secs(10), "{output}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        manager.stop_for_handoff(id).unwrap();
        let state = Command::new("ps").args(["-p", &pid.to_string(), "-o", "stat="]).output().unwrap();
        let state_text = String::from_utf8_lossy(&state.stdout);
        assert!(!state.status.success() || state_text.trim().is_empty() || matches!(state_text.trim().chars().next(), Some('Z' | 'X')), "owned task survived stop: {pid} {state_text}");
    }

    #[cfg(unix)]
    #[test]
    fn posix_reader_initialization_failure_stops_spawned_group() {
        let root = std::env::temp_dir().join(format!("pty-reader-failure-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let ready = root.join("ready.json");
        let pids = Arc::new(Mutex::new(Vec::<u32>::new()));
        let captured = Arc::clone(&pids); let wait_ready = ready.clone();
        FAIL_READER_INIT.with(|failure| *failure.borrow_mut() = Some(Box::new(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if let Ok(bytes) = std::fs::read(&wait_ready) {
                    if let Ok(values) = serde_json::from_slice::<Vec<u32>>(&bytes) { *captured.lock().unwrap() = values; break; }
                }
                assert!(std::time::Instant::now() < deadline); std::thread::sleep(std::time::Duration::from_millis(10));
            }
        })));
        let mut command = Command::new(which::which("node").unwrap());
        command.args(["-e", "process.on('SIGHUP',()=>{});const fs=require('node:fs');const c=require('node:child_process').spawn(process.execPath,['-e',\"process.on('SIGHUP',()=>{});process.send('ready');setInterval(()=>{},1000)\"],{stdio:['ignore','ignore','ignore','ipc']});c.on('message',()=>{fs.writeFileSync(process.argv[1]+'.tmp',JSON.stringify([process.pid,c.pid]));fs.renameSync(process.argv[1]+'.tmp',process.argv[1]);});setInterval(()=>{},1000);"]).arg(&ready);
        let manager = PtyManager::new();
        let error = manager.open_agent_program(&command, "reader-init-failure", false).err().unwrap();
        assert!(error.to_string().contains("reader 初始化失败"), "{error:#}");
        assert!(manager.sessions.lock().unwrap().is_empty(), "{error:#}");
        assert_eq!(pids.lock().unwrap().len(), 2);
        for pid in pids.lock().unwrap().iter() {
            let status = Command::new("ps").args(["-p", &pid.to_string(), "-o", "stat="]).output().unwrap();
            let state = String::from_utf8_lossy(&status.stdout);
            assert!(!status.status.success() || state.trim().is_empty() || matches!(state.trim().chars().next(), Some('Z' | 'X')), "initialization failure left active process {pid}: {state}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn posix_poll_retains_group_leader_until_surviving_child_is_stopped() {
        let manager = PtyManager::new();
        let mut command = Command::new(which::which("node").unwrap());
        command.args(["-e", "const c=require('node:child_process').spawn(process.execPath,['-e',\"process.on('SIGHUP',()=>{});process.send('ready');setInterval(()=>{},1000)\"],{stdio:['inherit','inherit','inherit','ipc']});c.on('message',()=>{console.log('GROUP_CHILD='+c.pid);c.disconnect();c.unref();process.exit(0);});"]);
        let id = manager.open_agent_program(&command, "posix-group-fixture", false).unwrap();
        let session = manager.session_arc(id).unwrap();
        let root_pid = session.lock().unwrap().child.lock().unwrap().process_id().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut output = String::new();
        let child_pid = loop {
            if let Some(chunk) = manager.poll_all().remove(&id) { output.push_str(&chunk.data); }
            let child = output.split("GROUP_CHILD=").nth(1)
                .and_then(|tail| tail.split(|c: char| !c.is_ascii_digit()).next())
                .and_then(|pid| pid.parse::<u32>().ok());
            if let Some(child) = child.filter(|_| !manager.is_running(id)) { break child; }
            assert!(std::time::Instant::now() < deadline, "{output}");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        for _ in 0..5 { manager.poll_all(); assert!(!manager.is_running(id)); }
        assert!(crate::acp::peek_process_exit(root_pid).unwrap().is_some(), "poll must not reap the PGID identity anchor");
        let active = Command::new("ps").args(["-p", &child_pid.to_string(), "-o", "stat="]).output().unwrap();
        let active_status = String::from_utf8_lossy(&active.stdout);
        assert!(active.status.success() && !active_status.trim().is_empty() && !matches!(active_status.trim().chars().next(), Some('Z' | 'X')), "fixture child must survive its session leader: {active_status}");
        manager.stop_for_handoff(id).unwrap();
        assert!(crate::acp::peek_process_exit(root_pid).is_err(), "only confirmed stop may reap the leader");
        let remaining = Command::new("ps").args(["-p", &child_pid.to_string(), "-o", "stat="]).output().unwrap();
        let status = String::from_utf8_lossy(&remaining.stdout);
        assert!(!remaining.status.success() || status.trim().is_empty() || matches!(status.trim().chars().next(), Some('Z' | 'X')), "child must not execute after stop: {status}");
    }

    #[cfg(windows)]
    #[test]
    fn kill_verifies_descendants_even_after_root_exits() {
        let manager = PtyManager::new();
        // ConPTY owns a job that can reap children with the root. Spawn the
        // isolated fixture outside that job to exercise a surviving descendant.
        let mut command = Command::new(which::which("node").unwrap());
        command.args(["-e", "const c=require('node:child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{detached:true,stdio:'ignore',windowsHide:true});console.log(c.pid);c.unref();"]);
        command.stdout(std::process::Stdio::piped());
        let (mut root, _root_identity) = crate::acp::spawn_owned_command(&mut command).unwrap();
        let root_pid = root.id();
        let mut output = String::new();
        std::io::Read::read_to_string(&mut root.stdout.take().unwrap(), &mut output).unwrap();
        assert!(root.wait().unwrap().success());
        let child_pid = output.trim().parse::<u32>().unwrap();
        let id = 903;
        fixture_session(&manager, id, "exited-root-fixture", Box::new(Vec::<u8>::new()), FixtureChild {
            handle: Some(std::os::windows::io::AsRawHandle::as_raw_handle(&root) as usize),
            pid: Some(root_pid), reject_kill: false, killed: Arc::new(AtomicBool::new(true)),
            released: Arc::new((Mutex::new(false), std::sync::Condvar::new())),
        });
        let running = || {
            let output = Command::new("tasklist.exe").args(["/FO", "CSV", "/NH"]).output().unwrap();
            assert!(output.status.success());
            String::from_utf8_lossy(&output.stdout).lines().any(|line| line.split("\",\"").nth(1).and_then(|field| field.trim_matches('"').parse::<u32>().ok()) == Some(child_pid))
        };
        assert!(running(), "fixture descendant must survive its parent");
        manager.kill(id).unwrap();
        assert!(!running(), "kill must not accept only the root exit");
        assert!(!manager.sessions.lock().unwrap().contains_key(&id));
    }

    #[cfg(windows)]
    #[test]
    fn checked_handoff_stop_waits_for_fixture_process_tree() {
        // 仅启动无业务操作的 Node 父子进程，不启动任何真实 Agent。
        let manager = PtyManager::new();
        let mut command = Command::new(which::which("node").unwrap());
        command.args(["-e", "require('node:child_process').spawn(process.execPath,['-e','setInterval(()=>{},1000)'],{stdio:'inherit'});setInterval(()=>{},1000)"]);
        let id = manager
            .open_agent_program(&command, "handoff-stop-fixture", false)
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(250));
        assert!(manager.is_running(id));
        manager.stop_for_handoff(id).unwrap();
        assert!(!manager.is_running(id));
        assert!(manager.write(id, b"not sent").is_err());
    }
}
