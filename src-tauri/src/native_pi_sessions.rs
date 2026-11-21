use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

use crate::acp::terminate_acp_process;
use crate::native_acp_sessions::{HarnessAttachment, HarnessChatMessage, HarnessSessionSnapshot};
use crate::sessions::{is_standalone_pi_chat, SessionManager, SessionRecord, SessionStatus};

#[cfg(test)]
#[path = "native_pi_communication_acceptance_tests.rs"]
mod communication_acceptance_tests;

/// pi 原生聊天（pi --mode rpc stdio JSONL）的事件通道。
const EVENT_NAME: &str = "pi-chat-update";

const PERMISSION_WAIT: Duration = Duration::from_secs(30 * 60);
const START_TIMEOUT: Duration = Duration::from_secs(45);
const CONTROL_TIMEOUT: Duration = Duration::from_secs(30);
const PROMPT_ACK_TIMEOUT: Duration = Duration::from_secs(60);
const PROMPT_MAX_CHARS: usize = 32_000;
/// pi RPC 是严格一行一 JSON（LF 分帧）；单行超限视为协议损坏。
const MAX_FRAME_BYTES: usize = 8 * 1024 * 1024;
const STDERR_RING_CHARS: usize = 12 * 1024;

/// pi 的写类工具经审批桥扩展弹出 confirm；其余对话框方法一律取消防挂起。
const FIRE_AND_FORGET_METHODS: &[&str] = &["notify", "setStatus", "setWidget", "setTitle", "set_editor_text"];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PiSessionLocator {
    cwd: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pi_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pi_session_file: Option<String>,
    history: Vec<HarnessChatMessage>,
    #[serde(default)]
    config_options: Vec<Value>,
    #[serde(default)]
    commands: Vec<Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct PersistedSessions {
    version: u32,
    sessions: HashMap<String, PiSessionLocator>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PiEvent {
    session_id: String,
    sequence: u64,
    kind: String,
    data: Value,
    record: Option<SessionRecord>,
    snapshot: Option<HarnessSessionSnapshot>,
}

struct PiClient {
    child: Mutex<Child>,
    _process_identity: crate::acp::ProcessIdentityGuard,
    stdin: Arc<Mutex<Option<ChildStdin>>>,
    process_id: Arc<AtomicU32>,
    stderr_tail: Arc<Mutex<String>>,
    pending: Arc<Mutex<HashMap<String, mpsc::SyncSender<std::result::Result<Value, String>>>>>,
    next_id: AtomicU64,
}

impl Drop for PiClient {
    fn drop(&mut self) { if let Err(error) = self.stop_checked() { eprintln!("[crewvela] Pi cleanup未确停，保留身份：{error:#}"); } }
}

type EventHandler = Box<dyn Fn(Value) + Send>;
type DisconnectHandler = Box<dyn Fn() + Send>;

impl PiClient {
    /// spawn `pi --mode rpc`（或测试对端）并启动读线程分帧路由。
    /// 非响应帧（事件 / extension_ui_request）交给 on_event；
    /// stdout EOF / 读错误交给 on_disconnect（仅触发一次）。
    fn spawn(
        mut command: Command,
        on_event: EventHandler,
        on_disconnect: DisconnectHandler,
    ) -> Result<Self> {
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let (mut child, process_identity) = crate::acp::spawn_owned_command(&mut command).context("启动 pi 进程失败")?;
        let pid = child.id();
        let stdin = child.stdin.take();
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdin = Arc::new(Mutex::new(stdin));
        let process_id = Arc::new(AtomicU32::new(pid));
        let stderr_tail = Arc::new(Mutex::new(String::new()));
        let pending: Arc<Mutex<HashMap<String, mpsc::SyncSender<std::result::Result<Value, String>>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        if let Some(stderr) = stderr {
            let tail = Arc::clone(&stderr_tail);
            std::thread::spawn(move || {
                let mut reader = std::io::BufReader::new(stderr);
                let mut buffer = [0u8; 4096];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) | Err(_) => break,
                        Ok(read) => {
                            if let Ok(mut tail) = tail.lock() {
                                tail.push_str(&String::from_utf8_lossy(&buffer[..read]));
                                let overflow = tail.chars().count().saturating_sub(STDERR_RING_CHARS);
                                if overflow > 0 {
                                    *tail = tail.chars().skip(overflow).collect();
                                }
                            }
                        }
                    }
                }
            });
        }
        let stdout = stdout.ok_or_else(|| anyhow!("pi 进程未提供 stdout"))?;
        let router_pending = Arc::clone(&pending);
        std::thread::spawn(move || {
            let mut reader = std::io::BufReader::new(stdout);
            let mut line: Vec<u8> = Vec::new();
            let failure = loop {
                match reader.read_until(b'\n', &mut line) {
                    Ok(0) => break None,
                    Ok(_) if line.len() > MAX_FRAME_BYTES => break Some("pi RPC 帧超出安全上限".to_string()),
                    Ok(_) if line.ends_with(b"\n") => {
                        if line.ends_with(b"\r\n") {
                            line.pop();
                        }
                        line.pop();
                        let frame = serde_json::from_slice::<Value>(&line);
                        line.clear();
                        match frame {
                            Ok(frame) => route_frame(&frame, &router_pending, &on_event),
                            Err(error) => break Some(format!("解析 pi RPC 帧失败：{error}")),
                        }
                    }
                    Ok(_) => {}
                    Err(error) => break Some(format!("读取 pi RPC 输出失败：{error}")),
                }
            };
            if let Some(message) = failure {
                on_event(json!({"type": "__crewvela_reader_failure", "message": message}));
            }
            on_disconnect();
        });
        Ok(Self {
            child: Mutex::new(child),
            _process_identity: process_identity,
            stdin,
            process_id,
            stderr_tail,
            pending,
            next_id: AtomicU64::new(1),
        })
    }

    /// 发送命令并按 id 等待响应。超时或进程退出时清理 pending 防泄漏。
    fn command(&self, timeout: Duration, payload: Value) -> Result<Value> {
        let id = self.next_id.fetch_add(1, Ordering::AcqRel).to_string();
        let mut payload = payload;
        if let Some(object) = payload.as_object_mut() {
            object.insert("id".into(), Value::String(id.clone()));
        }
        let (sender, receiver) = mpsc::sync_channel(1);
        self.pending
            .lock()
            .map_err(|_| anyhow!("pi 请求表锁已损坏"))?
            .insert(id.clone(), sender);
        self.write_raw(&payload).inspect_err(|_| {
            if let Ok(mut pending) = self.pending.lock() {
                pending.remove(&id);
            }
        })?;
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                if let Ok(mut pending) = self.pending.lock() {
                    pending.remove(&id);
                }
                return Err(anyhow!("等待 pi 响应超时（命令 {}）", payload_type(&payload)));
            }
            match receiver.recv_timeout(remaining.min(Duration::from_millis(100))) {
                Ok(Ok(data)) => return Ok(data),
                Ok(Err(message)) => return Err(anyhow!("pi 返回错误：{message}")),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if let Ok(mut pending) = self.pending.lock() {
                        pending.remove(&id);
                    }
                    return Err(anyhow!("pi 进程已退出，命令 {} 未获响应", payload_type(&payload)));
                }
            }
        }
    }

    fn write_raw(&self, payload: &Value) -> Result<()> {
        let mut line = serde_json::to_vec(payload).context("序列化 pi 命令失败")?;
        line.push(b'\n');
        let mut guard = self
            .stdin
            .lock()
            .map_err(|_| anyhow!("pi stdin 锁已损坏"))?;
        let stdin = guard.as_mut().ok_or_else(|| anyhow!("pi 进程 stdin 已关闭"))?;
        stdin
            .write_all(&line)
            .and_then(|_| stdin.flush())
            .context("写入 pi 命令失败")
    }

    fn stderr_tail(&self) -> String {
        self.stderr_tail
            .lock()
            .map(|tail| tail.clone())
            .unwrap_or_default()
    }

    fn stop_checked(&self) -> Result<()> {
        let mut child = self.child.lock().map_err(|_| anyhow!(crate::acp::UnverifiedStop("Pi child锁异常".into())))?;
        if !self._process_identity.is_verified() { crate::acp::stop_child_checked(&mut child)?; }
        child.wait().map_err(|error| anyhow!(crate::acp::UnverifiedStop(format!("回收Pi首领失败：{error}"))))?;
        let _ = self.process_id.compare_exchange(child.id(), 0, Ordering::AcqRel, Ordering::Acquire);
        Ok(())
    }

    /// 优雅停机：关 stdin（pi 收到 EOF 后 exit 0），2 秒内未退出再杀进程树。
    fn shutdown(&self) {
        if let Ok(mut guard) = self.stdin.lock() {
            *guard = None;
        }
        if let Ok(mut child) = self.child.lock() {
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline {
                match crate::acp::try_wait_child_preserving_identity(&mut child) {
                    Ok(Some(_)) => return,
                    Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                    Err(_) => break,
                }
            }
            let _ = child.kill();
        }
        terminate_acp_process(&self.process_id);
    }
}

fn payload_type(payload: &Value) -> &str {
    payload.get("type").and_then(Value::as_str).unwrap_or("unknown")
}

fn route_frame(
    frame: &Value,
    pending: &Arc<Mutex<HashMap<String, mpsc::SyncSender<std::result::Result<Value, String>>>>>,
    on_event: &EventHandler,
) {
    if frame.get("type").and_then(Value::as_str) == Some("response") {
        if let Some(id) = frame.get("id").and_then(Value::as_str) {
            let sender = pending.lock().ok().and_then(|mut map| map.remove(id));
            if let Some(sender) = sender {
                let result = if frame.get("success").and_then(Value::as_bool).unwrap_or(false) {
                    Ok(frame.get("data").cloned().unwrap_or(Value::Null))
                } else {
                    Err(frame
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("pi 命令执行失败")
                        .to_string())
                };
                let _ = sender.send(result);
                return;
            }
        }
        // 无 id 的响应（如 parse 错误）无宿主关联，静默忽略。
        return;
    }
    on_event(frame.clone());
}

struct LiveSession {
    lifecycle: Mutex<()>,
    publication: Mutex<()>,
    connection: Mutex<Option<Arc<PiClient>>>,
    process_id: Arc<AtomicU32>,
    busy: AtomicBool,
    connected: AtomicBool,
    cancelled: AtomicBool,
    generation: AtomicU64,
    pending_permissions: Mutex<HashMap<String, PendingPermission>>,
    active_turn_id: Mutex<Option<String>>,
    assistant_outcome: Mutex<Option<(SessionStatus, String)>>,
    persistence_failure: Mutex<Option<String>>,
    /// 当前轮是否已见过流式增量；未见增量时用 message_end 全量兜底。
    saw_stream_delta: AtomicBool,
    pending_delta: Mutex<(String, Instant)>,
    pending_reasoning: Mutex<String>,
    view: Mutex<LiveView>,
}

struct PendingPermission {
    sender: mpsc::SyncSender<bool>,
}

#[derive(Default)]
struct LiveView {
    sequence: u64,
    pending_permission: Option<Value>,
    tool_calls: HashMap<String, Value>,
}

impl LiveSession {
    fn new() -> Self {
        Self {
            lifecycle: Mutex::new(()),
            publication: Mutex::new(()),
            connection: Mutex::new(None),
            process_id: Arc::new(AtomicU32::new(0)),
            busy: AtomicBool::new(false),
            connected: AtomicBool::new(false),
            cancelled: AtomicBool::new(false),
            generation: AtomicU64::new(0),
            pending_permissions: Mutex::new(HashMap::new()),
            active_turn_id: Mutex::new(None),
            assistant_outcome: Mutex::new(None),
            persistence_failure: Mutex::new(None),
            saw_stream_delta: AtomicBool::new(false),
            pending_delta: Mutex::new((String::new(), Instant::now())),
            pending_reasoning: Mutex::new(String::new()),
            view: Mutex::new(LiveView::default()),
        }
    }
}

#[derive(Clone)]
pub struct NativePiSessions {
    path: PathBuf,
    store: Arc<Mutex<PersistedSessions>>,
    live: Arc<Mutex<HashMap<String, Arc<LiveSession>>>>,
    #[cfg(test)]
    pub(crate) test_peer: Option<PathBuf>,
}

impl NativePiSessions {
    pub fn open_at(path: PathBuf) -> Result<Self> {
        let persisted = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).with_context(|| "解析 Pi Chat 会话历史失败")?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => PersistedSessions {
                version: 1,
                ..PersistedSessions::default()
            },
            Err(error) => return Err(error).context("读取 Pi Chat 会话历史失败"),
        };
        if persisted.version != 1 {
            return Err(anyhow!("不支持的 Pi Chat 历史版本: {}", persisted.version));
        }
        Ok(Self {
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
                return Err(anyhow!("Pi Chat 会话已存在: {session_id}"));
            }
            store.sessions.insert(
                session_id.to_string(),
                PiSessionLocator {
                    cwd,
                    pi_session_id: None,
                    pi_session_file: None,
                    history: Vec::new(),
                    config_options: Vec::new(),
                    commands: Vec::new(),
                },
            );
            Ok(())
        })
    }

    pub(crate) fn contains(&self, id: &str) -> bool { self.store.lock().is_ok_and(|store| store.sessions.contains_key(id)) }
    pub(crate) fn has_worktree_runtime(&self, id: &str) -> Result<bool> {
        let live = self.live.lock().map_err(|_| anyhow!("原生会话锁损坏"))?.get(id).cloned();
        let Some(live) = live else { return Ok(false) };
        if live.process_id.load(Ordering::Acquire) != 0 || live.connected.load(Ordering::Acquire) || live.busy.load(Ordering::Acquire) { return Ok(true); }
        let connection = live.connection.try_lock().map_err(|_| anyhow!("原生连接仍在使用"))?;
        Ok(connection.is_some())
    }

    pub fn snapshot(&self, session_id: &str) -> Result<HarnessSessionSnapshot> {
        let live = self.live_session(session_id)?;
        let _publication = live.publication.lock().map_err(|_| anyhow!("Pi Chat 发布锁已损坏"))?;
        self.snapshot_unlocked(session_id, &live)
    }

    fn snapshot_unlocked(&self, session_id: &str, live: &LiveSession) -> Result<HarnessSessionSnapshot> {
        let locator = self.locator(session_id)?;
        let view = live.view.lock().map_err(|_| anyhow!("Pi Chat 视图锁已损坏"))?;
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
            control_state: Value::Null,
            context_usage: None,
            codex_goal: None,
            codex_goal_supported: false,
            codex_goal_action: None,
        })
    }

    pub fn start<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        sessions: &SessionManager,
        record: &SessionRecord,
    ) -> Result<HarnessSessionSnapshot> {
        crate::worktree_management::verify_start(record, sessions)?;
        sessions.claim_worktree_runtime(record)?;
        let _admission = app.try_state::<crate::runtime_admission::RuntimeAdmission>()
            .map(|gate| gate.admit()).transpose()?;
        if !is_standalone_pi_chat(record) {
            return Err(anyhow!("会话不是独立 Pi Chat 会话"));
        }
        let live = self.live_session(&record.id)?;
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("Pi Chat 生命周期锁已损坏"))?;
        let generation = live.generation.load(Ordering::Acquire);
        let result = self.start_connection(app.clone(), sessions, record, &live);
        if let Err(error) = &result {
            if live.generation.load(Ordering::Acquire) == generation
                && sessions.get(&record.id).is_some_and(|current| current.archived_at.is_none())
            {
                if let Ok(updated) = sessions.mark_native_runtime_status(
                    &record.id,
                    SessionStatus::Error,
                    None,
                    Some(error.to_string()),
                ) {
                    self.emit(&app, &live, &record.id, "error", json!({"message": error.to_string()}), Some(updated));
                }
            }
        }
        result
    }

    fn start_connection<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        sessions: &SessionManager,
        record: &SessionRecord,
        live: &Arc<LiveSession>,
    ) -> Result<HarnessSessionSnapshot> {
        // pi_chat 原生聊天持独立启停开关；探测/升级仍复用 pi CLI。
        crate::multi_agent::AgentRuntimeKind::PiChat.ensure_available()?;
        let locator = self.locator(&record.id)?;
        let cwd = canonical_directory(&record.cwd)?;
        if cwd != canonical_directory(&locator.cwd)? {
            return Err(anyhow!("Pi Chat 会话目录与持久化恢复定位不匹配"));
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
            .map_err(|_| anyhow!("Pi Chat 连接锁已损坏"))?;
        if connection_slot.is_some() {
            drop(connection_slot);
            return self.snapshot(&record.id);
        }
        crate::acp::terminate_acp_process_checked(&live.process_id)?;
        let generation = live.generation.load(Ordering::Acquire);
        live.cancelled.store(false, Ordering::Release);
        *live.assistant_outcome.lock().unwrap() = None;
        *live.persistence_failure.lock().unwrap() = None;
        live.saw_stream_delta.store(false, Ordering::Release);
        let mut resume_file = locator.pi_session_file.clone();
        let replacing_missing_empty_session = match locator.pi_session_file.as_deref() {
            Some(file) => match fs::metadata(file) {
                Ok(metadata) if metadata.is_file() => false,
                Ok(_) => {
                    return Err(anyhow!(
                        "Pi 原会话路径不是普通文件，未新建替代会话：{file}；原消息历史保留"
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    // pi 会在第一条 user/assistant 消息落盘；因此只有尚无任何本地历史时，
                    // 才能把这个尚未落盘的空 session 视作可重建。已有历史必须 fail closed。
                    if !locator.history.is_empty() {
                        return Err(anyhow!("Pi 原会话文件不存在，未新建替代会话：{file}；原消息历史保留"));
                    }
                    resume_file = None;
                    true
                }
                Err(error) => {
                    return Err(anyhow!(
                        "无法检查 Pi 原会话文件，未新建替代会话：{file}；原消息历史保留"
                    )
                    .context(error));
                }
            },
            None => false,
        };
        let command = self.build_command(&cwd, resume_file.as_deref())?;
        let manager = self.clone();
        let event_app = app.clone();
        let event_sessions = sessions.clone();
        let event_id = record.id.clone();
        let event_live = Arc::clone(live);
        let event_generation = generation;
        let on_event: EventHandler = Box::new(move |frame| {
            if event_live.generation.load(Ordering::Acquire) != event_generation {
                return;
            }
            manager.handle_frame(&event_app, &event_sessions, &event_id, &event_live, event_generation, &frame);
        });
        let disconnect_manager = self.clone();
        let disconnect_app = app.clone();
        let disconnect_sessions = sessions.clone();
        let disconnect_id = record.id.clone();
        let disconnect_live = Arc::clone(live);
        let disconnect_generation = generation;
        let on_disconnect: DisconnectHandler = Box::new(move || {
            disconnect_manager.handle_disconnect(&disconnect_app, &disconnect_sessions, &disconnect_id, &disconnect_live, disconnect_generation);
        });
        let client = match PiClient::spawn(command, on_event, on_disconnect) {
            Ok(client) => client,
            Err(error) => return Err(pi_spawn_error(error)),
        };
        if live.generation.load(Ordering::Acquire) != generation {
            client.stop_checked()?;
            return Err(anyhow!("Pi Chat 会话启动已取消"));
        }
        live.process_id.store(client.process_id.load(Ordering::Acquire), Ordering::Release);
        let client = Arc::new(client);
        let initialized = (|| -> Result<(Vec<Value>, SessionRecord)> {
        // 握手：get_state 验证会话身份，再拉取模型/思考级别/命令目录。
        let state = client.command(START_TIMEOUT, json!({"type": "get_state"})).map_err(|error| {
            let tail = client.stderr_tail();
            pi_spawn_error(error.context(format!("pi 握手失败{}", stderr_hint(&tail))))
        })?;
        let pi_session_id = state
            .get("sessionId")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("pi 握手响应缺少 sessionId"))?
            .to_string();
        let pi_session_file = state.get("sessionFile").and_then(Value::as_str).map(str::to_string);
        if let Some(expected) = &locator.pi_session_id {
            // 只对刚才确认过的“空历史 + 原文件缺失”重建放宽 session ID，
            // 所有实际文件恢复仍须匹配原身份。
            if !replacing_missing_empty_session && expected != &pi_session_id {
                    return Err(anyhow!(
                    "Pi Chat 会话恢复失败：pi 返回了不同的会话 ID（{pi_session_id} ≠ {expected}），请新建会话；原消息历史保留"
                ));
            }
        }
        let models = client.command(CONTROL_TIMEOUT, json!({"type": "get_available_models"})).map_err(|error| pi_spawn_error(error.context("获取 pi 模型目录失败")))?;
        let levels = client.command(CONTROL_TIMEOUT, json!({"type": "get_available_thinking_levels"})).map_err(|error| pi_spawn_error(error.context("获取 pi 思考级别失败")))?;
        let commands = client.command(CONTROL_TIMEOUT, json!({"type": "get_commands"})).map_err(|error| pi_spawn_error(error.context("获取 pi 命令目录失败")))?;
        let config_options = build_config_options(&state, &models, &levels);
        let command_list = parse_pi_commands(&commands);
        if live.generation.load(Ordering::Acquire) != generation {
            return Err(anyhow!("Pi Chat 会话启动已取消"));
        }
        self.mutate_store(|store| {
            let entry = store
                .sessions
                .get_mut(&record.id)
                .ok_or_else(|| anyhow!("Pi Chat 会话定位已删除"))?;
            if entry.cwd != locator.cwd {
                return Err(anyhow!("会话定位在启动期间发生变化"));
            }
            entry.pi_session_id = Some(pi_session_id.clone());
            entry.pi_session_file = pi_session_file.clone();
            entry.config_options = config_options.clone();
            entry.commands = command_list.clone();
            Ok(())
        })?;
        let updated_record = sessions.mark_native_runtime_status(&record.id, SessionStatus::Running, None, None)?;
        if live.generation.load(Ordering::Acquire) != generation {
            return Err(anyhow!("Pi Chat 会话启动已取消"));
        }
        Ok((config_options, updated_record))
        })();
        let (config_options, updated_record) = match initialized {
            Ok(value) => value,
            Err(error) => {
                if let Err(stop_error) = client.stop_checked() {
                    *connection_slot = Some(Arc::clone(&client));
                    live.connected.store(true, Ordering::Release);
                    live.busy.store(true, Ordering::Release);
                    return Err(stop_error);
                }
                live.process_id.store(0, Ordering::Release);
                return Err(error);
            }
        };
        let _publication = live.publication.lock().map_err(|_| anyhow!("Pi Chat 发布锁已损坏"))?;
        *connection_slot = Some(Arc::clone(&client));
        live.connected.store(true, Ordering::Release);
        self.emit_unlocked(
            &app,
            live,
            &record.id,
            "state",
            json!({"status": "running", "configOptions": config_options}),
            Some(updated_record),
        );
        drop(connection_slot);
        drop(_publication);
        self.snapshot(&record.id)
    }

    fn build_command(&self, cwd: &str, resume_file: Option<&str>) -> Result<Command> {
        let extension = self.ensure_bridge_extension()?;
        let mut command = match &self.test_peer_path() {
            #[cfg(test)]
            Some(peer) => {
                let mut command = Command::new(which::which("node").context("测试需要 node 可用")?);
                command.arg(peer);
                command
            }
            _ => {
                let path = crate::multi_agent::resolve_runtime_executable(
                    crate::multi_agent::AgentRuntimeKind::Pi,
                )
                .ok_or_else(|| {
                    anyhow!("未找到本机 pi CLI。请先安装 @earendil-works/pi-coding-agent（npm i -g）并在运行时管理中启用 Pi")
                })?;
                let mut command = crate::multi_agent::command_for_path(&path);
                crate::runtime_dependencies::with_managed_node(&mut command);
                command
            }
        };
        command.arg("--mode").arg("rpc").arg("-e").arg(&extension);
        if let Some(file) = resume_file {
            command.arg("--session").arg(file);
        }
        command.current_dir(cwd);
        Ok(command)
    }

    #[cfg(test)]
    fn test_peer_path(&self) -> Option<PathBuf> {
        self.test_peer.clone()
    }

    #[cfg(not(test))]
    fn test_peer_path(&self) -> Option<PathBuf> {
        None
    }

    /// include_str! 随写审批桥扩展（内容变化才落盘，原子 rename），
    /// 与 configure_pi_readiness 同一模式。
    fn ensure_bridge_extension(&self) -> Result<PathBuf> {
        let directory = self
            .path
            .parent()
            .ok_or_else(|| anyhow!("Pi Chat 存储目录无效"))?
            .join("runtime-extensions");
        fs::create_dir_all(&directory).context("创建 runtime-extensions 目录失败")?;
        let extension = directory.join("crewvela-pi-chat-bridge.mjs");
        let source = include_str!("pi-chat-bridge.mjs");
        if fs::read_to_string(&extension).ok().as_deref() != Some(source) {
            let temporary = directory.join(format!("crewvela-pi-chat-bridge.{}.tmp", uuid::Uuid::new_v4()));
            fs::write(&temporary, source).context("写入 pi 审批桥扩展失败")?;
            fs::rename(&temporary, &extension).context("落盘 pi 审批桥扩展失败")?;
        }
        Ok(extension)
    }

    /// pi 主事件分发：流式增量、工具事件、审批对话框、回合收口。
    fn handle_frame<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        sessions: &SessionManager,
        session_id: &str,
        live: &Arc<LiveSession>,
        generation: u64,
        frame: &Value,
    ) {
        let event_type = frame.get("type").and_then(Value::as_str).unwrap_or("");
        match event_type {
            "__crewvela_reader_failure" => {
                self.handle_disconnect(app, sessions, session_id, live, generation);
            }
            "message_update" => {
                let nested = frame.get("assistantMessageEvent");
                let nested_type = nested.and_then(|event| event.get("type")).and_then(Value::as_str);
                match nested_type {
                    Some("text_delta") => {
                        if let Some(delta) = nested.and_then(|event| event.get("delta")).and_then(Value::as_str) {
                            live.saw_stream_delta.store(true, Ordering::Release);
                            self.buffer_stream_delta(app, live, session_id, Some(delta), None);
                        }
                    }
                    Some("thinking_delta") => {
                        if let Some(delta) = nested.and_then(|event| event.get("delta")).and_then(Value::as_str) {
                            live.saw_stream_delta.store(true, Ordering::Release);
                            self.buffer_stream_delta(app, live, session_id, None, Some(delta));
                        }
                    }
                    _ => {}
                }
            }
            "message_end" => {
                let Some(message) = frame.get("message").filter(|message| message["role"] == "assistant") else { return; };
                // Observe the last assistant result even when deltas already supplied its body.
                // A later successful retry clears an earlier provider error.
                *live.assistant_outcome.lock().unwrap() = match message["stopReason"].as_str() {
                    Some("error") => Some((SessionStatus::Error, message["errorMessage"].as_str().unwrap_or("Pi 模型执行失败").to_string())),
                    Some("aborted") => Some((SessionStatus::Stopped, String::new())),
                    _ => None,
                };
                // 未见流式增量的 provider（或整段被吞）用权威终值兜底。
                if live.saw_stream_delta.load(Ordering::Acquire) { return; }
                let (text, reasoning) = extract_assistant_text(message);
                if !text.is_empty() || !reasoning.is_empty() {
                    self.buffer_stream_delta(app, live, session_id, Some(&text), Some(&reasoning));
                }
            }
            "tool_execution_start" | "tool_execution_update" | "tool_execution_end" => {
                let tool_call = tool_call_projection(frame, event_type);
                if let Some(turn_id) = live.active_turn_id.lock().ok().and_then(|id| id.clone()) {
                    if self.flush_assistant_delta(app, live, session_id, &turn_id).is_err() {
                        return;
                    }
                }
                self.emit(app, live, session_id, "tool_call", tool_call, None);
            }
            "agent_settled" => {
                if !live.cancelled.load(Ordering::Acquire) && live.assistant_outcome.lock().unwrap().is_none() {
                    if let Ok(locator) = self.locator(session_id) {
                        if let Some(message) = locator.history.iter().find(|message| message.role == "user") {
                            let model = locator.config_options.iter().find(|option| option["id"] == "model").and_then(|option| option["currentValue"].as_str()).map(str::to_string);
                            sessions.queue_title_summary_for_model(session_id, &message.text, locator.pi_session_file.clone(), model);
                        }
                    }
                }
                self.finish_turn(app, sessions, session_id, live, generation, true);
            }
            "thinking_level_changed" => {
                let level = frame.get("level").and_then(Value::as_str).unwrap_or_default();
                let updated = self.mutate_store(|store| {
                    let locator = store
                        .sessions
                        .get_mut(session_id)
                        .ok_or_else(|| anyhow!("Pi Chat 会话定位已删除"))?;
                    update_current_value(&mut locator.config_options, "thinking", level);
                    Ok(locator.config_options.clone())
                });
                if let Ok(options) = updated {
                    self.emit(app, live, session_id, "config_options", json!({"configOptions": options}), None);
                }
            }
            "extension_error" => {
                let message = format!(
                    "pi 扩展错误：{} ({})",
                    frame.get("error").and_then(Value::as_str).unwrap_or("未知错误"),
                    frame.get("extensionPath").and_then(Value::as_str).unwrap_or("unknown")
                );
                self.emit(app, live, session_id, "error", json!({"message": message}), None);
            }
            "extension_ui_request" => {
                self.handle_extension_ui(app, sessions, session_id, live, generation, frame);
            }
            _ => {}
        }
    }

    /// pi 进程退出 / RPC 流损坏：收口缓冲并转错误态（幂等，仅首次生效）。
    fn handle_disconnect<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        sessions: &SessionManager,
        session_id: &str,
        live: &Arc<LiveSession>,
        generation: u64,
    ) {
        let Ok(_lifecycle) = live.lifecycle.lock() else { return };
        if live.generation.load(Ordering::Acquire) != generation { return; }
        if live
            .connected
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        // Retire this connection generation so its delayed EOF/events cannot affect a resumed process.
        live.generation.fetch_add(1, Ordering::AcqRel);
        // The live slot and client have distinct PID atomics. Confirm the owned
        // job before taking the client, and clear the slot only after success.
        if let Err(error) = crate::acp::terminate_acp_process_checked(&live.process_id) {
            live.connected.store(true, Ordering::Release);
            live.busy.store(true, Ordering::Release);
            let failure = format!("Pi Chat 连接断开，但无法确认进程已停止：{error}");
            if let Ok(record) = sessions.mark_native_runtime_status(session_id, SessionStatus::Error, None, Some(failure.clone())) {
                self.emit(app, live, session_id, "error", json!({"message": failure}), Some(record));
            }
            return;
        }
        // flush_active_delta acquires publication itself; hold it only after flushing.
        let _ = self.flush_active_delta(app, live, session_id);
        let Ok(_publication) = live.publication.lock() else { return };
        let _ = self.interrupt_stream_messages(session_id);
        self.reject_pending_permissions(live);
        let disconnected = live.connection.lock().ok().and_then(|mut slot| slot.take());
        // Bad RPC can close the reader while the subprocess stays alive. Shut down
        // the retired process before allowing a resumed session to start.
        if let Some(client) = disconnected { client.shutdown(); }
        live.busy.store(false, Ordering::Release);
        if let Ok(mut turn) = live.active_turn_id.lock() {
            *turn = None;
        }
        let failure = "Pi Chat 连接已断开，请恢复会话".to_string();
        if let Ok(record) = sessions.mark_native_runtime_status(session_id, SessionStatus::Error, None, Some(failure.clone())) {
            self.emit_unlocked(app, live, session_id, "error", json!({"message": failure}), Some(record));
        }
    }

    fn buffer_stream_delta<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        live: &Arc<LiveSession>,
        session_id: &str,
        text: Option<&str>,
        reasoning: Option<&str>,
    ) {
        let turn_id = live
            .active_turn_id
            .lock()
            .ok()
            .map(|mut id| id.get_or_insert_with(|| uuid::Uuid::new_v4().to_string()).clone());
        let Some(turn_id) = turn_id else { return };
        let mut should_flush = false;
        if let Some(text) = text {
            if let Ok(mut pending) = live.pending_delta.lock() {
                if pending.0.is_empty() {
                    pending.1 = Instant::now();
                }
                pending.0.push_str(text);
                should_flush = pending.0.len() >= 1024 || pending.1.elapsed() >= Duration::from_millis(200);
            }
        }
        if let Some(reasoning) = reasoning {
            if let Ok(mut buffer) = live.pending_reasoning.lock() {
                buffer.push_str(reasoning);
                if buffer.len() >= 1024 {
                    should_flush = true;
                }
            }
        }
        if should_flush {
            // The flush preserves and publishes its error; a disk failure is not a user cancel.
            let _ = self.flush_assistant_delta(app, live, session_id, &turn_id);
        }
    }

    /// agent_settled：收口本轮（flush + 终态 + turn_end）。
    fn finish_turn<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        sessions: &SessionManager,
        session_id: &str,
        live: &Arc<LiveSession>,
        generation: u64,
        record_outcome: bool,
    ) {
        if live.generation.load(Ordering::Acquire) != generation { return; }
        let turn_id = live.active_turn_id.lock().ok().and_then(|id| id.clone());
        let assistant_outcome = live.assistant_outcome.lock().unwrap().take();
        let earlier_persistence_error = live.persistence_failure.lock().unwrap().take();
        let cancelled = live.cancelled.load(Ordering::Acquire) || assistant_outcome.as_ref().is_some_and(|(status, _)| *status == SessionStatus::Stopped);
        let assistant_failed = assistant_outcome.as_ref().is_some_and(|(status, _)| *status == SessionStatus::Error);
        let persistence_error = if let Some(turn_id) = &turn_id {
            let status = if assistant_failed || earlier_persistence_error.is_some() { "failed" } else if cancelled { "interrupted" } else { "completed" };
            self.flush_assistant_delta(app, live, session_id, turn_id)
                .and_then(|_| self.finalize_assistant_message(session_id, turn_id, status)).err()
        } else { None };
        if let Some(error) = &persistence_error {
            self.emit(app, live, session_id, "error", json!({"message": format!("保存 Pi 轮次失败：{error:#}")}), None);
        }
        if let Some((SessionStatus::Error, error)) = &assistant_outcome {
            self.emit(app, live, session_id, "error", json!({"message": error}), None);
        }
        let _publication = live.publication.lock().ok();
        live.busy.store(false, Ordering::Release);
        live.cancelled.store(false, Ordering::Release);
        *live.assistant_outcome.lock().unwrap() = None;
        *live.persistence_failure.lock().unwrap() = None;
        live.saw_stream_delta.store(false, Ordering::Release);
        if let Ok(mut turn) = live.active_turn_id.lock() {
            *turn = None;
        }
        if live.generation.load(Ordering::Acquire) != generation {
            return;
        }
        if record_outcome {
            if let Some(turn_id) = turn_id {
                let outcome = if persistence_error.is_some() || earlier_persistence_error.is_some() || assistant_failed { SessionStatus::Error }
                    else if cancelled { SessionStatus::Stopped }
                    else { SessionStatus::Completed };
                if let Err(error) = sessions.record_native_turn_outcome(session_id, &turn_id, outcome) {
                    self.emit_unlocked(app, live, session_id, "error", json!({"message": format!("记录轮次事件失败：{error:#}")}), None);
                }
            }
        }
        self.emit_unlocked(app, live, session_id, "turn_end", json!({"cancelled": cancelled}), None);
    }

    fn handle_extension_ui<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        sessions: &SessionManager,
        session_id: &str,
        live: &Arc<LiveSession>,
        generation: u64,
        frame: &Value,
    ) {
        let dialog_id = frame.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        if dialog_id.is_empty() {
            return;
        }
        let method = frame.get("method").and_then(Value::as_str).unwrap_or("");
        if FIRE_AND_FORGET_METHODS.contains(&method) {
            return;
        }
        // 审批桥扩展只用 confirm；其它对话框方法一律取消，防扩展挂起。
        if method != "confirm" {
            self.answer_dialog(live, &dialog_id, Some(false));
            return;
        }
        let (tool_call, reason) = permission_projection(frame);
        let request_id = uuid::Uuid::new_v4().to_string();
        let (sender, receiver) = mpsc::sync_channel(1);
        if let Ok(mut requests) = live.pending_permissions.lock() {
            requests.insert(request_id.clone(), PendingPermission { sender });
        }
        let record = sessions.mark_native_runtime_status(session_id, SessionStatus::Waiting, Some(reason), None);
        if let Ok(record) = record {
            self.emit(app, live, session_id, "state", json!({"status": "waiting"}), Some(record));
        }
        self.emit(
            app,
            live,
            session_id,
            "permission_request",
            json!({
                "requestId": request_id,
                "toolCall": tool_call,
                "options": [
                    {"optionId": "allow_once", "name": "允许一次", "kind": "allow_once"},
                    {"optionId": "reject_once", "name": "拒绝", "kind": "reject_once"}
                ],
                "default": "reject_once"
            }),
            None,
        );
        let manager = self.clone();
        let waiter_app = app.clone();
        let waiter_sessions = sessions.clone();
        let waiter_id = session_id.to_string();
        let waiter_live = Arc::clone(live);
        let waiter_dialog = dialog_id.clone();
        let waiter_request = request_id.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let decision = loop {
                if waiter_live.generation.load(Ordering::Acquire) != generation
                    || waiter_live.cancelled.load(Ordering::Acquire)
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
            if let Ok(mut requests) = waiter_live.pending_permissions.lock() {
                requests.remove(&waiter_request);
            }
            manager.answer_dialog(&waiter_live, &waiter_dialog, Some(decision));
            if waiter_live.generation.load(Ordering::Acquire) == generation
                && waiter_live.connected.load(Ordering::Acquire)
            {
                if let Ok(record) = waiter_sessions.mark_native_runtime_status(
                    &waiter_id,
                    SessionStatus::Running,
                    None,
                    None,
                ) {
                    manager.emit(
                        &waiter_app,
                        &waiter_live,
                        &waiter_id,
                        "state",
                        json!({"status": "running"}),
                        Some(record),
                    );
                }
            }
        });
    }

    /// 把对话框应答回写 pi（extension_ui_response），并清理视图中的审批卡片。
    fn answer_dialog(&self, live: &LiveSession, dialog_id: &str, confirmed: Option<bool>) {
        let response = json!({"type": "extension_ui_response", "id": dialog_id, "confirmed": confirmed.unwrap_or(false)});
        if let Ok(slot) = live.connection.lock() {
            if let Some(client) = slot.as_ref() {
                let _ = client.write_raw(&response);
            }
        }
    }

    pub fn answer_permission(&self, session_id: &str, request_id: &str, allow: bool) -> Result<()> {
        let live = self.live_session(session_id)?;
        let permission = live
            .pending_permissions
            .lock()
            .map_err(|_| anyhow!("Pi Chat 审批状态锁已损坏"))?
            .remove(request_id)
            .ok_or_else(|| anyhow!("审批请求已结束或不存在"))?;
        if let Ok(mut view) = live.view.lock() {
            view.pending_permission = None;
        }
        permission
            .sender
            .send(allow)
            .map_err(|_| anyhow!("审批请求已取消"))
    }

    fn reject_pending_permissions(&self, live: &LiveSession) {
        if let Ok(mut requests) = live.pending_permissions.lock() {
            for (_, permission) in requests.drain() {
                let _ = permission.sender.send(false);
            }
        }
    }

    pub fn cancel_turn(&self, session_id: &str) -> Result<bool> {
        let live = self.live_session(session_id)?;
        if !live.busy.load(Ordering::Acquire) {
            return Ok(false);
        }
        live.cancelled.store(true, Ordering::Release);
        self.reject_pending_permissions(&live);
        let client = live
            .connection
            .lock()
            .map_err(|_| anyhow!("Pi Chat 连接锁已损坏"))?
            .clone();
        if let Some(client) = client {
            client.command(CONTROL_TIMEOUT, json!({"type": "abort"}))?;
        }
        Ok(true)
    }

    pub fn set_config(&self, session_id: &str, id: &str, value: &str) -> Result<HarnessSessionSnapshot> {
        let live = self.live_session(session_id)?;
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("Pi Chat 生命周期锁已损坏"))?;
        if live.busy.load(Ordering::Acquire) {
            return Err(anyhow!("Pi Chat 正在处理消息，暂不能切换配置"));
        }
        let client = live
            .connection
            .lock()
            .map_err(|_| anyhow!("Pi Chat 连接锁已损坏"))?
            .clone()
            .ok_or_else(|| anyhow!("Pi Chat 会话尚未连接"))?;
        match id {
            "thinking" => {
                client.command(CONTROL_TIMEOUT, json!({"type": "set_thinking_level", "level": value}))?;
            }
            "model" => {
                let (provider, model_id) = value
                    .split_once('/')
                    .ok_or_else(|| anyhow!("模型值无效（应为 provider/modelId）：{value}"))?;
                client.command(
                    CONTROL_TIMEOUT,
                    json!({"type": "set_model", "provider": provider, "modelId": model_id}),
                )?;
            }
            other => return Err(anyhow!("Pi Chat 不支持该配置项：{other}")),
        }
        let options = self.mutate_store(|store| {
            let locator = store
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| anyhow!("Pi Chat 会话定位已删除"))?;
            update_current_value(&mut locator.config_options, id, value);
            Ok(locator.config_options.clone())
        })?;
        drop(options);
        self.snapshot(session_id)
    }

    pub fn send_message_with_attachments<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        sessions: &SessionManager,
        session_id: &str,
        message: &str,
        paths: &[String],
    ) -> Result<()> {
        validate_cc_session_id(session_id)?;
        let message = message.trim().to_string();
        let attachments = normalize_attachments(paths)?;
        if (message.is_empty() && attachments.is_empty()) || message.chars().count() > PROMPT_MAX_CHARS {
            return Err(anyhow!("消息不能为空且不能超过 {PROMPT_MAX_CHARS} 个字符"));
        }
        let live = self.live_session(session_id)?;
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("Pi Chat 生命周期锁已损坏"))?;
        let _publication = live.publication.lock().map_err(|_| anyhow!("Pi Chat 发布锁已损坏"))?;
        // 附件先行读取（图片转 base64，其余转文本引用），失败时不进入 busy 态。
        let mut prompt_message = message.clone();
        let mut images = Vec::new();
        for attachment in &attachments {
            match read_image_attachment(&attachment.path) {
                Ok(Some(image)) => images.push(image),
                Ok(None) => {
                    prompt_message.push_str(&format!("\n\n[附件] {}", attachment.path));
                }
                Err(error) => return Err(error),
            }
        }
        if prompt_message.trim().is_empty() && !images.is_empty() {
            prompt_message = "请处理我附加的文件。".to_string();
        }
        if live.busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() {
            return Err(anyhow!("Pi Chat 正在处理上一条消息"));
        }
        let client = live
            .connection
            .lock()
            .map_err(|_| anyhow!("Pi Chat 连接锁已损坏"))?
            .clone();
        let Some(client) = client else {
            live.busy.store(false, Ordering::Release);
            return Err(anyhow!("Pi Chat 会话尚未连接，请先恢复会话"));
        };
        let turn_id = uuid::Uuid::new_v4().to_string();
        if let Err(error) = self.append_user_message(session_id, &turn_id, &message, &attachments) {
            live.busy.store(false, Ordering::Release);
            return Err(error);
        }
        *live
            .active_turn_id
            .lock()
            .map_err(|_| anyhow!("Pi Chat 消息状态锁已损坏"))? = Some(turn_id.clone());
        live.cancelled.store(false, Ordering::Release);
        *live.assistant_outcome.lock().unwrap() = None;
        *live.persistence_failure.lock().unwrap() = None;
        live.saw_stream_delta.store(false, Ordering::Release);
        let generation = live.generation.load(Ordering::Acquire);
        self.emit_unlocked(&app, &live, session_id, "user_message", json!({"id": turn_id, "text": message}), None);
        let manager = self.clone();
        let worker_app = app.clone();
        let worker_id = session_id.to_string();
        let worker_live = Arc::clone(&live);
        let worker_sessions = sessions.clone();
        std::thread::spawn(move || {
            if worker_live.generation.load(Ordering::Acquire) != generation {
                return;
            }
            let mut payload = json!({"type": "prompt", "message": prompt_message});
            if !images.is_empty() {
                if let Some(object) = payload.as_object_mut() {
                    object.insert("images".into(), serde_json::to_value(&images).unwrap_or(Value::Array(Vec::new())));
                }
            }
            let disposition = client.command(PROMPT_ACK_TIMEOUT, payload);
            let Ok(_lifecycle) = worker_live.lifecycle.lock() else { return };
            if worker_live.generation.load(Ordering::Acquire) != generation {
                return;
            }
            match disposition {
                Ok(data) => {
                    if data.get("disposition").and_then(Value::as_str) == Some("handled") {
                        // 命令被扩展就地处理，不会有 agent run / agent_settled。
                        manager.finish_turn(&worker_app, &worker_sessions, &worker_id, &worker_live, generation, false);
                    }
                }
                Err(error) => {
                    worker_live.busy.store(false, Ordering::Release);
                    if let Ok(mut turn) = worker_live.active_turn_id.lock() {
                        *turn = None;
                    }
                    manager.emit(
                        &worker_app,
                        &worker_live,
                        &worker_id,
                        "error",
                        json!({"message": format!("发送到 pi 失败：{error:#}")}),
                        None,
                    );
                }
            }
        });
        Ok(())
    }

    pub(crate) fn stop_for_handoff<R: tauri::Runtime>(&self, session_id: &str, sessions: &SessionManager, app: &AppHandle<R>) -> Result<SessionRecord> { self.stop_inner(session_id, sessions, app, true) }
    pub fn stop<R: tauri::Runtime>(&self, session_id: &str, sessions: &SessionManager, app: &AppHandle<R>) -> Result<SessionRecord> { self.stop_inner(session_id, sessions, app, false) }
    fn stop_inner<R: tauri::Runtime>(&self, session_id: &str, sessions: &SessionManager, app: &AppHandle<R>, _verified: bool) -> Result<SessionRecord> {
        self.stop_inner_with(session_id, sessions, app, crate::acp::terminate_acp_process_checked)
    }

    fn stop_inner_with<R: tauri::Runtime>(&self, session_id: &str, sessions: &SessionManager, app: &AppHandle<R>, terminate: impl Fn(&AtomicU32) -> Result<()>) -> Result<SessionRecord> {
        let record = sessions.get(session_id).ok_or_else(|| anyhow!("未找到会话: {session_id}"))?;
        if !is_standalone_pi_chat(&record) {
            return Err(anyhow!("会话不是独立 Pi Chat 会话"));
        }
        let live = self.live_session(session_id)?;
        live.generation.fetch_add(1, Ordering::AcqRel);
        live.cancelled.store(true, Ordering::Release);
        self.reject_pending_permissions(&live);
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("Pi Chat 生命周期锁已损坏"))?;
        terminate(&live.process_id)?;
        let client = live.connection.lock().map_err(|_| anyhow!("Pi Chat 连接锁已损坏"))?.take();
        if let Some(client) = &client {
            client.shutdown();
        }
        let flush_result = self
            .flush_active_delta(app, &live, session_id)
            .and_then(|_| self.interrupt_stream_messages(session_id));
        let _publication = live.publication.lock().map_err(|_| anyhow!("Pi Chat 发布锁已损坏"))?;
        live.busy.store(false, Ordering::Release);
        live.connected.store(false, Ordering::Release);
        if flush_result.is_ok() {
            *live.active_turn_id.lock().map_err(|_| anyhow!("Pi Chat 消息状态锁已损坏"))? = None;
        }
        let updated = sessions.mark_stopped(session_id)?;
        self.emit_unlocked(app, &live, session_id, "state", json!({"status": "stopped"}), Some(updated.clone()));
        flush_result?;
        Ok(updated)
    }

    pub fn stop_all<R: tauri::Runtime>(&self, sessions: &SessionManager, app: &AppHandle<R>) -> Result<()> {
        let ids = self
            .store
            .lock()
            .map_err(|_| anyhow!("Pi Chat 存储锁已损坏"))?
            .sessions
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        for id in ids {
            if sessions.get(&id).is_some_and(|record| is_standalone_pi_chat(&record)) {
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
        let _lifecycle = live.lifecycle.lock().map_err(|_| anyhow!("Pi Chat 生命周期锁已损坏"))?;
        crate::acp::terminate_acp_process_checked(&live.process_id)?;
        let client = live.connection.lock().map_err(|_| anyhow!("Pi Chat 连接锁已损坏"))?.take();
        if let Some(client) = &client {
            client.shutdown();
        }
        live.busy.store(false, Ordering::Release);
        live.connected.store(false, Ordering::Release);
        let mut store = self.store.lock().map_err(|_| anyhow!("Pi Chat 存储锁已损坏"))?;
        let mut next = store.clone();
        next.sessions.remove(session_id);
        save_atomic(&self.path, &next)?;
        if let Err(error) = delete_record() {
            if let Err(restore_error) = save_atomic(&self.path, &store) {
                return Err(anyhow!("删除 CrewVela 会话失败：{error:#}；恢复 Pi Chat 历史也失败：{restore_error:#}"));
            }
            return Err(error);
        }
        *store = next;
        drop(store);
        self.live
            .lock()
            .map_err(|_| anyhow!("Pi Chat 会话表锁已损坏"))?
            .remove(session_id);
        Ok(())
    }

    fn append_user_message(&self, session_id: &str, id: &str, text: &str, attachments: &[HarnessAttachment]) -> Result<()> {
        self.mutate_store(|store| {
            let locator = store
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| anyhow!("Pi Chat 会话定位不存在"))?;
            locator.history.push(HarnessChatMessage {
                id: id.into(),
                role: "user".into(),
                text: text.into(),
                created_at: now_millis(),
                attachments: attachments.to_vec(),
                reasoning: String::new(),
                stream_status: String::new(),
            });
            Ok(())
        })
    }

    fn interrupt_stream_messages(&self, session_id: &str) -> Result<()> {
        self.mutate_store(|store| {
            // A failed launch can leave an outer record without Pi history.
            // Missing history needs no settlement; save/flush errors still propagate.
            if let Some(locator) = store.sessions.get_mut(session_id) {
                for message in &mut locator.history {
                    if message.stream_status == "streaming" {
                        message.stream_status = "interrupted".into();
                    }
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
                .ok_or_else(|| anyhow!("Pi Chat 会话定位不存在"))?;
            if let Some(message) = locator
                .history
                .iter_mut()
                .find(|message| message.id == turn_id && message.role == "assistant")
            {
                message.text.push_str(text);
                message.reasoning.push_str(reasoning);
            } else {
                locator.history.push(HarnessChatMessage {
                    id: turn_id.into(),
                    role: "assistant".into(),
                    text: text.into(),
                    created_at: now_millis(),
                    attachments: Vec::new(),
                    reasoning: reasoning.into(),
                    stream_status: String::new(),
                });
            }
            Ok(())
        })
    }

    fn finalize_assistant_message(&self, session_id: &str, turn_id: &str, status: &str) -> Result<()> {
        self.mutate_store(|store| {
            let locator = store
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| anyhow!("Pi Chat 会话定位不存在"))?;
            if let Some(message) = locator
                .history
                .iter_mut()
                .find(|message| message.id == turn_id && message.role == "assistant")
            {
                message.stream_status = status.into();
            }
            Ok(())
        })
    }

    fn flush_active_delta<R: tauri::Runtime>(&self, app: &AppHandle<R>, live: &LiveSession, session_id: &str) -> Result<()> {
        let turn_id = live
            .active_turn_id
            .lock()
            .map_err(|_| anyhow!("Pi Chat 消息状态锁已损坏"))?
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
        let _publication = live.publication.lock().map_err(|_| anyhow!("Pi Chat 发布锁已损坏"))?;
        let text = {
            let mut pending = live
                .pending_delta
                .lock()
                .map_err(|_| anyhow!("Pi Chat 回复缓冲锁已损坏"))?;
            let text = std::mem::take(&mut pending.0);
            pending.1 = Instant::now();
            text
        };
        let reasoning = live
            .pending_reasoning
            .lock()
            .map_err(|_| anyhow!("Pi Chat 思考缓冲锁已损坏"))
            .map(|mut buffer| std::mem::take(&mut *buffer))?;
        if text.is_empty() && reasoning.is_empty() {
            return Ok(());
        }
        if let Err(error) = self.append_assistant_delta(session_id, turn_id, &text, &reasoning) {
            *live.persistence_failure.lock().unwrap() = Some(format!("保存 Pi Chat 回复失败：{error:#}"));
            if let Ok(mut pending) = live.pending_delta.lock() {
                pending.0 = format!("{text}{}", pending.0);
            }
            if let Ok(mut buffer) = live.pending_reasoning.lock() {
                *buffer = format!("{reasoning}{}", buffer);
            }
            self.emit_unlocked(app, live, session_id, "error", json!({"message": format!("保存 Pi Chat 回复失败：{error:#}")}), None);
            return Err(error);
        }
        self.emit_unlocked(app, live, session_id, "assistant_delta", json!({"id": turn_id, "text": text, "reasoning": reasoning}), None);
        Ok(())
    }

    fn emit<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        live: &LiveSession,
        session_id: &str,
        kind: &str,
        data: Value,
        record: Option<SessionRecord>,
    ) {
        let _publication = live.publication.lock().expect("Pi Chat publication lock poisoned");
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
            } else if kind == "turn_end" || kind == "error" {
                view.pending_permission = None;
            } else if kind == "state" && data.get("status").and_then(Value::as_str) == Some("running") {
                view.pending_permission = None;
            } else if kind == "state" && data.get("status").and_then(Value::as_str) == Some("stopped") {
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
                eprintln!("[crewvela] 公共会话 Pi 事件持久化失败：{error:#}");
            }
        }
        let _ = app.emit(
            EVENT_NAME,
            PiEvent {
                session_id: session_id.to_string(),
                sequence,
                kind: kind.to_string(),
                data,
                record,
                snapshot: self.snapshot_unlocked(session_id, live).ok(),
            },
        );
    }

    fn live_session(&self, session_id: &str) -> Result<Arc<LiveSession>> {
        let mut live = self
            .live
            .lock()
            .map_err(|_| anyhow!("Pi Chat 会话表锁已损坏"))?;
        Ok(Arc::clone(
            live.entry(session_id.to_string())
                .or_insert_with(|| Arc::new(LiveSession::new())),
        ))
    }

    fn locator(&self, session_id: &str) -> Result<PiSessionLocator> {
        validate_cc_session_id(session_id)?;
        self.store
            .lock()
            .map_err(|_| anyhow!("Pi Chat 存储锁已损坏"))?
            .sessions
            .get(session_id)
            .cloned()
            .ok_or_else(|| anyhow!("未找到 Pi Chat 会话定位"))
    }

    fn mutate_store<T>(&self, mutate: impl FnOnce(&mut PersistedSessions) -> Result<T>) -> Result<T> {
        let mut store = self.store.lock().map_err(|_| anyhow!("Pi Chat 存储锁已损坏"))?;
        let mut next = store.clone();
        let result = mutate(&mut next)?;
        save_atomic(&self.path, &next)?;
        *store = next;
        Ok(result)
    }
}

impl LiveSession {}

/// pi 启动/握手错误附加安装与 stderr 提示。
fn pi_spawn_error(error: anyhow::Error) -> anyhow::Error {
    if error.downcast_ref::<crate::acp::UnverifiedStop>().is_some() { return error; }
    let message = format!("{error:#}");
    anyhow!("Pi Chat 启动失败：{message}。请确认本机已安装 pi（npm i -g @earendil-works/pi-coding-agent）且已配置模型凭证")
}

fn stderr_hint(tail: &str) -> String {
    let trimmed = tail.trim();
    if trimmed.is_empty() {
        String::new()
    } else {
        let last: String = trimmed.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
        format!("；stderr：{last}")
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

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn save_atomic(path: &Path, store: &PersistedSessions) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).context("创建 Pi Chat 会话目录失败")?;
    }
    let bytes = serde_json::to_vec_pretty(store).context("序列化 Pi Chat 会话失败")?;
    let temp = path.with_extension(format!("pi-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .context("创建 Pi Chat 会话临时文件失败")?;
        file.write_all(&bytes).context("写入 Pi Chat 会话失败")?;
        drop(file);
        fs::rename(&temp, path).context("保存 Pi Chat 会话失败")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

// ACP resource links 同语义：附件保持文件身份，由 agent 自行打开。
fn normalize_attachments(paths: &[String]) -> Result<Vec<HarnessAttachment>> {
    if paths.len() > 16 {
        return Err(anyhow!("每条消息最多附加 16 个文件"));
    }
    let mut files = Vec::new();
    for input in paths {
        let path = fs::canonicalize(input).with_context(|| format!("附件不可访问：{input}"))?;
        if !path.is_file() {
            return Err(anyhow!("附件必须是文件：{input}"));
        }
        let path_string = crate::acp::normalize_harness_path(&path)?;
        if files.iter().any(|file: &HarnessAttachment| file.path == path_string) {
            continue;
        }
        let uri = tauri::Url::from_file_path(&path_string)
            .map_err(|_| anyhow!("无法转换附件路径：{input}"))?
            .to_string();
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("附件名称不是有效 UTF-8"))?
            .to_string();
        files.push(HarnessAttachment { path: path_string, uri, name });
    }
    Ok(files)
}

const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

const IMAGE_MIME_TYPES: &[(&str, &str)] = &[
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
    ("gif", "image/gif"),
    ("webp", "image/webp"),
];

/// 图片附件转 pi 的 ImageContent（base64）；非图片返回 None 由调用方改为文本引用。
fn read_image_attachment(path: &str) -> Result<Option<Value>> {
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    let Some((_, mime)) = IMAGE_MIME_TYPES.iter().find(|(suffix, _)| *suffix == extension) else {
        return Ok(None);
    };
    use base64::Engine as _;
    let file = fs::File::open(path).with_context(|| format!("读取图片附件失败：{path}"))?;
    let metadata = file.metadata().with_context(|| format!("读取图片附件信息失败：{path}"))?;
    if metadata.len() > MAX_IMAGE_BYTES { return Err(anyhow!("图片附件超过20 MiB上限：{path}")); }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_IMAGE_BYTES + 1).read_to_end(&mut bytes).with_context(|| format!("读取图片附件失败：{path}"))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES { return Err(anyhow!("图片附件在读取期间超过20 MiB上限：{path}")); }
    let data = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Ok(Some(json!({"type": "image", "data": data, "mimeType": mime})))
}

pub(crate) fn conversation_config_options(cwd: &str, model: Option<&str>, thinking: Option<&str>) -> Result<Vec<Value>> {
    let kind = crate::multi_agent::AgentRuntimeKind::Pi;
    let executable = crate::multi_agent::resolve_runtime_executable_checked(kind).map_err(anyhow::Error::msg)?;
    let mut command = crate::multi_agent::command_for_path(&executable);
    command.current_dir(cwd).args(["--mode", "rpc", "--no-session", "--no-extensions", "--no-skills",
        "--no-mcp", "--no-context-files", "--no-prompt-templates"]);
    if let Some(model) = model { command.args(["--model", model]); }
    if let Some(thinking) = thinking { command.args(["--thinking", thinking]); }
    let client = PiClient::spawn(command, Box::new(|_| {}), Box::new(|| {}))?;
    let outcome = (|| {
        let state = client.command(START_TIMEOUT, json!({"type":"get_state"}))?;
        let models = client.command(CONTROL_TIMEOUT, json!({"type":"get_available_models"}))?;
        let levels = client.command(CONTROL_TIMEOUT, json!({"type":"get_available_thinking_levels"}))?;
        let options = build_config_options(&state, &models, &levels);
        for (id, value) in [("model", model), ("thinking", thinking)] {
            if let Some(value) = value {
                anyhow::ensure!(options.iter().any(|option| option["id"] == id && option["currentValue"] == value),
                    "Pi 未确认本轮的模型或推理级别，请重新选择");
            }
        }
        Ok(options)
    })();
    client.stop_checked()?;
    outcome
}

/// 组装 configOptions（model + thinking），形状与 kimi fixture/ACP 一致。
fn build_config_options(state: &Value, models: &Value, levels: &Value) -> Vec<Value> {
    let mut options = Vec::new();
    let model_list: Vec<Value> = models
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| {
                    let provider = model.get("provider").and_then(Value::as_str)?;
                    let id = model.get("id").and_then(Value::as_str)?;
                    let name = model.get("name").and_then(Value::as_str).unwrap_or(id);
                    Some(json!({"name": name, "value": format!("{provider}/{id}")}))
                })
                .collect()
        })
        .unwrap_or_default();
    let current_model = state
        .get("model")
        .and_then(|model| {
            let provider = model.get("provider").and_then(Value::as_str)?;
            let id = model.get("id").and_then(Value::as_str)?;
            Some(format!("{provider}/{id}"))
        })
        .unwrap_or_default();
    if !model_list.is_empty() {
        options.push(json!({
            "id": "model", "type": "select", "currentValue": current_model, "options": model_list,
        }));
    }
    let level_list: Vec<Value> = levels
        .get("levels")
        .and_then(Value::as_array)
        .map(|levels| {
            levels
                .iter()
                .filter_map(|level| level.as_str())
                .map(|level| json!({"name": level, "value": level}))
                .collect()
        })
        .unwrap_or_default();
    if !level_list.is_empty() {
        let current = state
            .get("thinkingLevel")
            .and_then(Value::as_str)
            .unwrap_or("off");
        options.push(json!({
            "id": "thinking", "type": "select", "currentValue": current, "options": level_list,
        }));
    }
    options
}

fn update_current_value(options: &mut [Value], id: &str, value: &str) {
    for option in options.iter_mut() {
        if option.get("id").and_then(Value::as_str) == Some(id) {
            if let Some(object) = option.as_object_mut() {
                object.insert("currentValue".into(), Value::String(value.to_string()));
            }
        }
    }
}

/// pi 的 get_commands 响应 → 前端 commands 数组（name/description/inputHint 防御式映射）。
fn parse_pi_commands(response: &Value) -> Vec<Value> {
    response
        .get("commands")
        .and_then(Value::as_array)
        .map(|commands| {
            commands
                .iter()
                .filter_map(|command| {
                    let name = command.get("name").and_then(Value::as_str)?;
                    let mut mapped = json!({
                        "name": name,
                        "description": command.get("description").and_then(Value::as_str).unwrap_or(""),
                    });
                    let hint = command
                        .pointer("/input/hint")
                        .and_then(Value::as_str)
                        .or_else(|| command.get("inputHint").and_then(Value::as_str));
                    if let Some(hint) = hint {
                        if let Some(object) = mapped.as_object_mut() {
                            object.insert("inputHint".into(), Value::String(hint.to_string()));
                        }
                    }
                    Some(mapped)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// tool_execution_* 事件 → 前端 tool_call 投影。
fn tool_call_projection(frame: &Value, event_type: &str) -> Value {
    let id = frame.get("toolCallId").and_then(Value::as_str).unwrap_or("unknown");
    let name = frame.get("toolName").and_then(Value::as_str).unwrap_or("工具调用");
    let status = match event_type {
        "tool_execution_start" => "in_progress",
        "tool_execution_end" => {
            if frame.get("isError").and_then(Value::as_bool).unwrap_or(false) {
                "failed"
            } else {
                "completed"
            }
        }
        _ => "in_progress",
    };
    let mut projection = json!({
        "toolCallId": id,
        "title": name,
        "status": status,
    });
    if let Some(args) = frame.get("args") {
        if let Some(object) = projection.as_object_mut() {
            object.insert("rawInput".into(), args.clone());
        }
    }
    if let Some(output) = frame.get("partialResult").or_else(|| frame.get("result")) {
        if let Some(object) = projection.as_object_mut() {
            object.insert("rawOutput".into(), output.clone());
        }
    }
    projection
}

/// 解析审批桥扩展的 confirm 信封（title JSON），失败时回退原始文本。
fn permission_projection(frame: &Value) -> (Value, String) {
    let title = frame.get("title").and_then(Value::as_str).unwrap_or("工具调用");
    if let Ok(envelope) = serde_json::from_str::<Value>(title) {
        if envelope.get("kind").and_then(Value::as_str) == Some("crewvela-permission") {
            let tool_name = envelope.get("toolName").and_then(Value::as_str).unwrap_or("工具调用");
            let tool_call = json!({
                "toolCallId": envelope.get("toolCallId").cloned().unwrap_or(Value::Null),
                "title": format!("{tool_name} 工具调用"),
                "kind": tool_name,
                "rawInput": envelope.get("input").cloned().unwrap_or(Value::Null),
            });
            return (tool_call, format!("{tool_name} 工具调用待确认"));
        }
    }
    (json!({"title": title}), "工具调用待确认".to_string())
}

/// message_end 的权威 assistant 内容（TextContent / ThinkingContent 拼接）。
fn extract_assistant_text(message: &Value) -> (String, String) {
    let mut text = String::new();
    let mut reasoning = String::new();
    if let Some(blocks) = message.get("content").and_then(Value::as_array) {
        for block in blocks {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    if let Some(chunk) = block.get("text").and_then(Value::as_str) {
                        text.push_str(chunk);
                    }
                }
                Some("thinking") => {
                    if let Some(chunk) = block.get("thinking").and_then(Value::as_str) {
                        reasoning.push_str(chunk);
                    }
                }
                _ => {}
            }
        }
    }
    (text, reasoning)
}

#[cfg(test)]
mod tests {

    #[test]
    fn image_attachment_is_bounded_before_reading_and_keeps_normal_payload() {
        use base64::Engine as _;
        let root = std::env::temp_dir().join(format!("pi-image-limit-{}", uuid::Uuid::new_v4())); fs::create_dir_all(&root).unwrap();
        let oversized = root.join("oversized.png");
        let file = fs::File::create(&oversized).unwrap(); file.set_len(MAX_IMAGE_BYTES + 1).unwrap(); drop(file);
        assert!(read_image_attachment(oversized.to_str().unwrap()).unwrap_err().to_string().contains("20 MiB"));
        let path = root.join("normal.png");
        let bytes = base64::engine::general_purpose::STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/l1sAAAAASUVORK5CYII=").unwrap();
        fs::write(&path, &bytes).unwrap();
        let image = read_image_attachment(path.to_str().unwrap()).unwrap().unwrap();
        assert_eq!(image["mimeType"], "image/png");
        assert_eq!(base64::engine::general_purpose::STANDARD.decode(image["data"].as_str().unwrap()).unwrap(), bytes);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_native_stop_preserves_client_and_process_identity() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        let live = manager.live_session(&record.id).unwrap();
        let pid = live.process_id.load(Ordering::Acquire);
        let error = manager.stop_inner_with(&record.id, &sessions, app.handle(), |_| Err(anyhow!("fixture tree kill denied"))).unwrap_err();
        assert!(error.to_string().contains("kill denied"));
        assert_eq!(live.process_id.load(Ordering::Acquire), pid);
        assert!(live.connection.lock().unwrap().is_some());
        assert!(live.connected.load(Ordering::Acquire));
        assert_ne!(sessions.get(&record.id).unwrap().status, SessionStatus::Stopped);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    use super::*;
    use tauri::Manager;

    fn fixture() -> (PathBuf, NativePiSessions, SessionManager, SessionRecord, tauri::App<tauri::test::MockRuntime>) {
        let root = std::env::temp_dir().join(format!("cc-pi-chat-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sessions = SessionManager::open_at(root.join("records.json"), root.join("events")).unwrap();
        let record = sessions
            .create_runtime(crate::sessions::NewRuntimeSession {
                runtime_kind: "pi_chat".into(),
                cwd: root.display().to_string(),
                project_id: None,
            })
            .unwrap();
        let mut manager = NativePiSessions::open_at(root.join("pi-chat-sessions.json")).unwrap();
        manager.test_peer = Some(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/pi-chat-agent.mjs"));
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
    fn closed_runtime_admission_rejects_pi_start_before_spawning() {
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

    fn startup_failure_peer(root: &Path, gated: bool) -> PathBuf {
        let path = root.join("startup-error.mjs");
        let marker = serde_json::to_string(&root.join("startup-process.json").display().to_string()).unwrap();
        let release = serde_json::to_string(&root.join("release-error").display().to_string()).unwrap();
        let source = format!(r#"import fs from 'node:fs';import net from 'node:net';import readline from 'node:readline';
const marker={marker}, release={release}, gated={gated};let ready=false,queue=[];
const send=v=>process.stdout.write(JSON.stringify(v)+'\n');
const reply=f=>{{if(f.type==='get_state')send({{type:'response',id:f.id,command:f.type,success:true,data:{{sessionId:'pi-startup-error'}}}});else if(f.type==='get_available_models'){{const fail=()=>send({{type:'response',id:f.id,command:f.type,success:false,error:'fixture model catalog failed'}});if(!gated)fail();else{{const timer=setInterval(()=>{{if(fs.existsSync(release)){{clearInterval(timer);fail()}}}},5)}}}}}};
const server=net.createServer(s=>s.end());server.listen(0,'127.0.0.1',()=>{{fs.writeFileSync(marker,JSON.stringify({{pid:process.pid,port:server.address().port}}));ready=true;for(const f of queue)reply(f);queue=[]}});
readline.createInterface({{input:process.stdin}}).on('line',l=>{{const f=JSON.parse(l);if(ready)reply(f);else queue.push(f)}});
// Intentionally ignore stdin EOF: cleanup must terminate this owned process.
setInterval(()=>{{}},1000);
"#);
        fs::write(&path, source).unwrap(); path
    }

    #[test]
    fn failed_startup_after_handshake_terminates_the_unpublished_client() {
        let (root, mut manager, sessions, record, app) = fixture();
        manager.test_peer = Some(startup_failure_peer(&root, false));
        let error = manager.start(app.handle().clone(), &sessions, &record).unwrap_err();
        assert!(error.to_string().contains("模型目录"), "{error:#}");
        let marker: Value = serde_json::from_slice(&fs::read(root.join("startup-process.json")).unwrap()).unwrap();
        let address = std::net::SocketAddr::from(([127,0,0,1], marker["port"].as_u64().unwrap() as u16));
        assert!(std::net::TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_err(), "failed startup left its server running");
        let live = manager.live_session(&record.id).unwrap();
        assert!(live.connection.lock().unwrap().is_none());
        assert_eq!(live.process_id.load(Ordering::Acquire), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn failed_startup_with_rejected_stop_keeps_client_and_blocks_a_second_spawn() {
        let (root, mut manager, sessions, record, app) = fixture();
        manager.test_peer = Some(startup_failure_peer(&root, true));
        let worker = { let manager=manager.clone(); let sessions=sessions.clone(); let record=record.clone(); let app=app.handle().clone();
            std::thread::spawn(move || manager.start(app, &sessions, &record)) };
        let started=Instant::now();
        let marker: Value = loop {
            if let Some(value) = fs::read(root.join("startup-process.json")).ok().and_then(|raw| serde_json::from_slice(&raw).ok()) { break value; }
            assert!(started.elapsed()<Duration::from_secs(5)); std::thread::sleep(Duration::from_millis(5));
        };
        let pid=marker["pid"].as_u64().unwrap() as u32;
        let denied=crate::acp::TestStopFailureGuard::new(pid);
        fs::write(root.join("release-error"), "release").unwrap();
        let error=worker.join().unwrap().unwrap_err();
        assert!(error.downcast_ref::<crate::acp::UnverifiedStop>().is_some(), "{error:#}");
        let live=manager.live_session(&record.id).unwrap();
        assert_eq!(live.process_id.load(Ordering::Acquire),pid);
        assert!(live.connection.lock().unwrap().is_some());
        let same=manager.start(app.handle().clone(),&sessions,&record).unwrap();
        assert!(same.busy && same.connected);
        assert_eq!(live.process_id.load(Ordering::Acquire),pid);
        drop(denied);
        manager.stop(&record.id,&sessions,app.handle()).unwrap();
        let address=std::net::SocketAddr::from(([127,0,0,1],marker["port"].as_u64().unwrap() as u16));
        assert!(std::net::TcpStream::connect_timeout(&address,Duration::from_millis(300)).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    fn wait_for(manager: &NativePiSessions, id: &str, predicate: impl Fn(&HarnessSessionSnapshot) -> bool) -> HarnessSessionSnapshot {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = manager.snapshot(id).unwrap();
            if predicate(&snapshot) {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "pi chat state did not settle: {snapshot:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn pi_chat_broken_rpc_retires_the_still_running_bridge_process() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.app_handle().clone(), &sessions, &record).unwrap();
        let live = manager.live_session(&record.id).unwrap();
        let client = live.connection.lock().unwrap().clone().unwrap();
        assert!(client.child.lock().unwrap().try_wait().unwrap().is_none());
        let generation = live.generation.load(Ordering::Acquire);
        manager.handle_frame(app.app_handle(), &sessions, &record.id, &live, generation,
            &json!({"type":"__crewvela_reader_failure","message":"invalid RPC JSON"}));
        let deadline = Instant::now() + Duration::from_secs(3);
        while client.child.lock().unwrap().try_wait().unwrap().is_none() {
            assert!(Instant::now() < deadline, "broken RPC child was left alive");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pi_chat_bridge_eof_flushes_active_turn_without_deadlocking() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.app_handle().clone(), &sessions, &record).unwrap();
        let live = manager.live_session(&record.id).unwrap();
        let generation = live.generation.load(Ordering::Acquire);
        // A delayed EOF from an earlier process must leave the new connection intact.
        manager.handle_disconnect(app.app_handle(), &sessions, &record.id, &live, generation.wrapping_sub(1));
        assert!(live.connected.load(Ordering::Acquire));
        manager.send_message_with_attachments(app.app_handle().clone(), &sessions, &record.id, "断连回归", &[]).unwrap();
        wait_for(&manager, &record.id, |snapshot| snapshot.pending_permission.is_some());
        live.pending_delta.lock().unwrap().0 = "EOF 前尚未发布的内容".into();
        live.busy.store(true, Ordering::Release);
        let client = live.connection.lock().unwrap().clone().unwrap();
        client.shutdown(); // Real bridge subprocess closes stdout and invokes on_disconnect.
        let deadline = Instant::now() + Duration::from_secs(5);
        // busy is cleared before disconnect status publication finishes. Wait
        // for both guards too, so a brief cleanup lock is not mistaken for a
        // deadlock. The same deadline still detects a permanently held guard.
        while live.busy.load(Ordering::Acquire)
            || live.lifecycle.try_lock().is_err()
            || live.publication.try_lock().is_err()
        {
            assert!(Instant::now() < deadline, "EOF cleanup deadlocked");
            std::thread::sleep(Duration::from_millis(10));
        }
        let snapshot = manager.snapshot(&record.id).unwrap();
        assert!(!snapshot.connected);
        assert!(snapshot.history.iter().any(|message| message.text.contains("EOF 前尚未发布的内容")));
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Error);
        drop(client);
        assert_eq!(live.process_id.load(Ordering::Acquire), 0);
        manager.start(app.app_handle().clone(), &sessions, &record).unwrap();
        manager.handle_disconnect(app.app_handle(), &sessions, &record.id, &live, generation);
        assert!(manager.snapshot(&record.id).unwrap().connected);
        manager.stop(&record.id, &sessions, app.app_handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pi_last_assistant_outcome_controls_automation_even_after_streamed_text() {
        for streamed in [false, true] {
            for (reason, retry_success, expected) in [
                ("error", false, SessionStatus::Error),
                ("aborted", false, SessionStatus::Stopped),
                ("stop", false, SessionStatus::Completed),
                ("error", true, SessionStatus::Completed),
            ] {
                let (root, manager, sessions, record, app) = fixture();
                sessions.mark_native_runtime_status(&record.id, SessionStatus::Running, None, None).unwrap();
                let live = manager.live_session(&record.id).unwrap();
                let turn = uuid::Uuid::new_v4().to_string();
                *live.active_turn_id.lock().unwrap() = Some(turn);
                live.busy.store(true, Ordering::Release);
                live.saw_stream_delta.store(streamed, Ordering::Release);
                let generation = live.generation.load(Ordering::Acquire);
                manager.handle_frame(app.handle(), &sessions, &record.id, &live, generation, &json!({"type":"message_end","message":{"role":"assistant","stopReason":reason,"errorMessage":"model failed","content":[]}}));
                if retry_success {
                    manager.handle_frame(app.handle(), &sessions, &record.id, &live, generation, &json!({"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[]}}));
                }
                manager.handle_frame(app.handle(), &sessions, &record.id, &live, generation, &json!({"type":"agent_settled"}));
                manager.handle_frame(app.handle(), &sessions, &record.id, &live, generation, &json!({"type":"agent_settled"}));
                let terminal = sessions.drain_transitions().into_iter().filter(|event| matches!(event.to, SessionStatus::Completed | SessionStatus::Error | SessionStatus::Stopped)).collect::<Vec<_>>();
                assert_eq!(terminal.len(), 1, "{reason} streamed={streamed} retry={retry_success}");
                assert_eq!(terminal[0].to, expected);
                assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
                fs::remove_dir_all(root).unwrap();
            }
        }
    }

    #[test]
    fn pi_stream_flush_failure_is_failed_even_if_storage_recovers_before_settlement() {
        let (root, manager, sessions, record, app) = fixture();
        sessions.mark_native_runtime_status(&record.id, SessionStatus::Running, None, None).unwrap();
        let live = manager.live_session(&record.id).unwrap();
        *live.active_turn_id.lock().unwrap() = Some(uuid::Uuid::new_v4().to_string());
        live.busy.store(true, Ordering::Release);
        let saved = fs::read(&manager.path).unwrap();
        fs::remove_file(&manager.path).unwrap();
        fs::create_dir(&manager.path).unwrap();
        let generation = live.generation.load(Ordering::Acquire);
        manager.handle_frame(app.handle(), &sessions, &record.id, &live, generation, &json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"x".repeat(1024)}}));
        assert!(live.persistence_failure.lock().unwrap().is_some());
        assert!(!live.cancelled.load(Ordering::Acquire), "storage failure must not become user cancellation");
        fs::remove_dir(&manager.path).unwrap();
        fs::write(&manager.path, saved).unwrap();
        manager.handle_frame(app.handle(), &sessions, &record.id, &live, generation, &json!({"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[]}}));
        manager.handle_frame(app.handle(), &sessions, &record.id, &live, generation, &json!({"type":"agent_settled"}));
        let terminal = sessions.drain_transitions().into_iter().filter(|event| matches!(event.to, SessionStatus::Completed | SessionStatus::Error | SessionStatus::Stopped)).collect::<Vec<_>>();
        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].to, SessionStatus::Error);
        assert_eq!(manager.snapshot(&record.id).unwrap().history.last().unwrap().stream_status, "failed");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pi_reply_persistence_failure_never_publishes_success() {
        let (root, manager, sessions, record, app) = fixture();
        sessions.mark_native_runtime_status(&record.id, SessionStatus::Running, None, None).unwrap();
        let live = manager.live_session(&record.id).unwrap();
        *live.active_turn_id.lock().unwrap() = Some(uuid::Uuid::new_v4().to_string());
        live.pending_delta.lock().unwrap().0 = "unsaved reply".into();
        live.busy.store(true, Ordering::Release);
        fs::remove_file(&manager.path).unwrap();
        fs::create_dir(&manager.path).unwrap();
        manager.finish_turn(app.handle(), &sessions, &record.id, &live, live.generation.load(Ordering::Acquire), true);
        let terminal = sessions.drain_transitions().into_iter().filter(|event| matches!(event.to, SessionStatus::Completed | SessionStatus::Error)).collect::<Vec<_>>();
        assert_eq!(terminal.len(), 1);
        assert_eq!(terminal[0].to, SessionStatus::Error);
        assert!(!live.busy.load(Ordering::Acquire));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pi_chat_streams_approves_and_updates_config() {
        let (root, manager, sessions, record, app) = fixture();
        let snapshot = manager.start(app.app_handle().clone(), &sessions, &record).unwrap();
        assert!(snapshot.connected);
        assert_eq!(snapshot.config_options.len(), 2);
        assert!(snapshot.commands.iter().any(|command| command.get("name").and_then(Value::as_str) == Some("init")));
        manager
            .send_message_with_attachments(app.app_handle().clone(), &sessions, &record.id, "你好", &[])
            .unwrap();
        let waiting = wait_for(&manager, &record.id, |snapshot| snapshot.pending_permission.is_some());
        let permission = waiting.pending_permission.clone().unwrap();
        assert_eq!(
            permission.pointer("/toolCall/kind").and_then(Value::as_str),
            Some("bash")
        );
        manager.answer_permission(&record.id, permission.get("requestId").and_then(Value::as_str).unwrap(), true).unwrap();
        let done = wait_for(&manager, &record.id, |snapshot| {
            !snapshot.busy
                && snapshot
                    .history
                    .iter()
                    .any(|message| message.role == "assistant" && message.text.contains("批准后的结尾"))
        });
        assert!(done.tool_calls.iter().any(|tool| {
            tool.get("toolCallId").and_then(Value::as_str) == Some("bash-1")
                && tool.get("status").and_then(Value::as_str) == Some("completed")
        }));
        assert_eq!(sessions.drain_transitions().into_iter().filter(|event| event.to == SessionStatus::Completed).count(), 1);
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
        let updated = manager.set_config(&record.id, "thinking", "low").unwrap();
        assert_eq!(
            updated
                .config_options
                .iter()
                .find(|option| option.get("id").and_then(Value::as_str) == Some("thinking"))
                .and_then(|option| option.get("currentValue"))
                .and_then(Value::as_str),
            Some("low")
        );
        manager.stop(&record.id, &sessions, &app.app_handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pi_chat_resume_keeps_session_id_and_history() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.app_handle().clone(), &sessions, &record).unwrap();
        manager
            .send_message_with_attachments(app.app_handle().clone(), &sessions, &record.id, "第一轮", &[])
            .unwrap();
        let waiting = wait_for(&manager, &record.id, |snapshot| snapshot.pending_permission.is_some());
        manager
            .answer_permission(
                &record.id,
                waiting.pending_permission.unwrap().get("requestId").and_then(Value::as_str).unwrap(),
                true,
            )
            .unwrap();
        wait_for(&manager, &record.id, |snapshot| {
            !snapshot.busy && snapshot.history.iter().any(|message| message.text.contains("批准后的结尾"))
        });
        manager.stop(&record.id, &sessions, &app.app_handle()).unwrap();
        let history_len = manager.snapshot(&record.id).unwrap().history.len();
        assert!(history_len >= 2);
        let persisted: Value = serde_json::from_slice(&fs::read(root.join("pi-chat-sessions.json")).unwrap()).unwrap();
        assert_eq!(
            persisted.pointer("/sessions").and_then(Value::as_object).unwrap()
                .get(&record.id).unwrap()
                .get("piSessionId").and_then(Value::as_str),
            Some("pi-fake-new-1")
        );
        // 恢复：fixture 会校验 --session 参数并回写 marker。
        manager.start(app.app_handle().clone(), &sessions, &record).unwrap();
        let marker = root.join("pi-resumed-marker.json");
        assert!(marker.exists(), "fixture 第二次启动应收到 --session 参数");
        let resumed = manager.snapshot(&record.id).unwrap();
        assert!(resumed.connected);
        assert_eq!(resumed.history.len(), history_len, "恢复不得重复历史");
        manager.stop(&record.id, &sessions, &app.app_handle()).unwrap();
        // 会话文件缺失时拒绝启动，保护本地历史。
        let mut broken: Value = serde_json::from_slice(&fs::read(root.join("pi-chat-sessions.json")).unwrap()).unwrap();
        *broken
            .pointer_mut(&format!("/sessions/{}/piSessionFile", record.id))
            .unwrap() = Value::String(root.join("missing.jsonl").display().to_string());
        fs::write(root.join("pi-chat-sessions.json"), serde_json::to_vec(&broken).unwrap()).unwrap();
        let reopened = NativePiSessions::open_at(root.join("pi-chat-sessions.json")).unwrap();
        let locator_before = serde_json::to_value(reopened.locator(&record.id).unwrap()).unwrap();
        let persisted_before = fs::read(root.join("pi-chat-sessions.json")).unwrap();
        let error = reopened
            .start(app.app_handle().clone(), &sessions, &record)
            .expect_err("缺失会话文件应报错");
        assert!(format!("{error:#}").contains("Pi 原会话文件不存在"));
        assert_eq!(
            serde_json::to_value(reopened.locator(&record.id).unwrap()).unwrap(),
            locator_before,
            "有历史时缺失文件不得改变恢复定位"
        );
        assert_eq!(
            fs::read(root.join("pi-chat-sessions.json")).unwrap(),
            persisted_before,
            "有历史时缺失文件不得改写持久化定位"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pi_chat_empty_session_restarts_if_pi_has_not_created_its_file() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.app_handle().clone(), &sessions, &record).unwrap();
        manager.stop(&record.id, &sessions, &app.app_handle()).unwrap();

        let store_path = root.join("pi-chat-sessions.json");
        let initial_locator = manager.locator(&record.id).unwrap();
        assert!(initial_locator.history.is_empty());
        let original_file = initial_locator.pi_session_file.clone().unwrap();
        let original_pi_session_id = initial_locator.pi_session_id.clone().unwrap();
        assert!(
            !Path::new(&original_file).exists(),
            "仅握手 get_state 后 Pi 不应已创建空 session 文件"
        );

        let reopened = NativePiSessions::open_at(store_path).unwrap();
        let result = reopened.start(app.app_handle().clone(), &sessions, &record);
        let (connected, history_empty, error) = match result {
            Ok(snapshot) => {
                let values = (snapshot.connected, snapshot.history.is_empty(), None);
                reopened.stop(&record.id, &sessions, &app.app_handle()).unwrap();
                values
            }
            Err(error) => (false, false, Some(format!("{error:#}"))),
        };
        let replacement_locator = reopened.locator(&record.id).unwrap();
        let persisted: Value = serde_json::from_slice(&fs::read(root.join("pi-chat-sessions.json")).unwrap()).unwrap();
        let persisted_locator = persisted
            .pointer("/sessions")
            .and_then(Value::as_object)
            .unwrap()
            .get(&record.id)
            .unwrap();
        let resumed_marker = root.join("pi-resumed-marker.json").exists();
        fs::remove_dir_all(&root).unwrap();

        assert!(connected, "空 Pi 会话在 Pi 尚未创建 sessionFile 时应能恢复：{error:?}");
        assert!(history_empty, "重新启动空会话不得引入历史");
        assert!(!resumed_marker, "缺失文件的空会话应以新空 session 启动，不传 --session");
        assert_ne!(replacement_locator.pi_session_file.as_deref(), Some(original_file.as_str()));
        assert_ne!(replacement_locator.pi_session_id.as_deref(), Some(original_pi_session_id.as_str()));
        assert_eq!(
            persisted_locator.get("piSessionId").and_then(Value::as_str),
            replacement_locator.pi_session_id.as_deref(),
            "替代空 session 的 ID 应持久化"
        );
        assert_eq!(
            persisted_locator.get("piSessionFile").and_then(Value::as_str),
            replacement_locator.pi_session_file.as_deref(),
            "替代空 session 的文件定位应持久化"
        );
    }

    #[test]
    fn pi_chat_empty_session_rejects_directory_at_session_file_path() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.app_handle().clone(), &sessions, &record).unwrap();
        manager.stop(&record.id, &sessions, &app.app_handle()).unwrap();

        let session_file = manager.locator(&record.id).unwrap().pi_session_file.unwrap();
        fs::create_dir(&session_file).unwrap();
        let store_path = root.join("pi-chat-sessions.json");
        let persisted_before = fs::read(&store_path).unwrap();
        let reopened = NativePiSessions::open_at(store_path.clone()).unwrap();
        let locator_before = serde_json::to_value(reopened.locator(&record.id).unwrap()).unwrap();
        let error = reopened
            .start(app.app_handle().clone(), &sessions, &record)
            .expect_err("目录占用了原 session 文件路径时必须拒绝重建");
        assert!(format!("{error:#}").contains("Pi 原会话路径不是普通文件"));
        assert_eq!(
            serde_json::to_value(reopened.locator(&record.id).unwrap()).unwrap(),
            locator_before,
            "非文件路径不得改变恢复定位"
        );
        assert_eq!(
            fs::read(&store_path).unwrap(),
            persisted_before,
            "非文件路径不得改写持久化定位"
        );
        assert!(
            !root.join("pi-resumed-marker.json").exists(),
            "拒绝目录路径时不得启动 pi 进程"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn pi_chat_cancel_interrupts_inflight_turn() {
        let (root, manager, sessions, record, app) = fixture();
        manager.start(app.app_handle().clone(), &sessions, &record).unwrap();
        manager
            .send_message_with_attachments(app.app_handle().clone(), &sessions, &record.id, "长任务", &[])
            .unwrap();
        wait_for(&manager, &record.id, |snapshot| snapshot.busy);
        assert!(manager.cancel_turn(&record.id).unwrap());
        let settled = wait_for(&manager, &record.id, |snapshot| !snapshot.busy);
        assert!(settled.connected, "取消后应保持连接");
        assert!(sessions.drain_transitions().iter().all(|event| !matches!(event.to, SessionStatus::Completed | SessionStatus::Error)), "cancelled Pi turn must not fire completion/failure automations");
        manager.stop(&record.id, &sessions, &app.app_handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
