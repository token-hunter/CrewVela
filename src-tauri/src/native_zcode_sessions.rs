//! Independent ZCode conversations. ZCode owns the Agent loop and native history.
use crate::{
    multi_agent::{
        zcode::{self, DesktopConnection},
        AgentRuntimeKind,
    },
    sessions::{is_standalone_zcode, SessionManager, SessionRecord, SessionStatus},
};
use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, Manager};

// Desktop path references use the native AttachmentRef contract. Keep the
// conservative composer limits explicit, and revalidate at send time.
pub(crate) fn prepare_attachments(paths: &[String]) -> Result<Vec<Value>> {
    if paths.len() > 8 {
        bail!("每次最多添加 8 个附件");
    }
    let mut refs = Vec::new();
    for input in paths {
        let path = Path::new(input);
        if !path.is_absolute() {
            bail!("附件必须使用绝对路径");
        }
        let path = fs::canonicalize(path).context("附件不存在或无法访问")?;
        let metadata = fs::metadata(&path).context("读取附件信息失败")?;
        if !metadata.is_file() {
            bail!("附件必须是普通文件");
        }
        if metadata.len() > 20 * 1024 * 1024 {
            bail!("CrewVela 附件上限为每个 20 MiB");
        }
        let name = path
            .file_name()
            .and_then(|v| v.to_str())
            .context("附件文件名不可用")?;
        let ext = path
            .extension()
            .and_then(|v| v.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mime = match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "pdf" => "application/pdf",
            "mp4" => "video/mp4",
            "webm" => "video/webm",
            "json" => "application/json",
            "txt" | "md" | "rs" | "ts" | "tsx" | "js" | "py" | "csv" | "log" => "text/plain",
            _ => "application/octet-stream",
        };
        refs.push(json!({"ref":path.to_str().context("附件路径不可用")?,"fileName":name,"mime":mime,"bytes":metadata.len()}));
    }
    Ok(refs)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Locator {
    cwd: String,
    native_session_id: Option<String>,
    rows: Vec<Value>,
    #[serde(default)]
    model_selection: Option<Value>,
    #[serde(default)]
    permission_mode: Option<String>,
    #[serde(default)]
    queued_inputs: Vec<Value>,
    #[serde(default)]
    recoverable_inputs: Vec<Value>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Store {
    version: u32,
    sessions: HashMap<String, Locator>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ZCodeSnapshot {
    pub session_id: String,
    pub connected: bool,
    pub busy: bool,
    pub rows: Vec<Value>,
    pub pending_permission: Option<Value>,
    pub sequence: u64,
    pub error: Option<String>,
    pub models: Vec<zcode::ModelChoice>,
    pub selected_model_id: Option<String>,
    pub permission_mode: Option<String>,
    pub queued_inputs: Vec<Value>,
    pub recoverable_inputs: Vec<Value>,
    pub queue_auto_drain: bool,
    pub can_queue: bool,
    pub input_routing: Option<String>,
    pub child_detail: Option<Value>,
    /// /side|/btw 辅助对话分栏（childSessionId/rows/loading/error），None = 未打开。
    pub side_detail: Option<Value>,
    /// 上下文用量（used/size/cache/breakdown），None = 尚未收到 usage 事实。
    pub context_usage: Option<Value>,
}
#[derive(Default)]
struct View {
    connected: bool,
    busy: bool,
    pending: Option<(Value, mpsc::SyncSender<bool>)>,
    sequence: u64,
    error: Option<String>,
    models: Vec<zcode::ModelChoice>,
    selected_model_id: Option<String>,
    permission_mode: Option<String>,
    queued_inputs: Vec<Value>,
    queue_auto_drain: bool,
    can_queue: bool,
    input_routing: Option<String>,
    child_detail: Option<Value>,
    side_detail: Option<Value>,
    context_usage: Option<Value>,
}
enum Action {
    Edit(u64, String, String, mpsc::SyncSender<std::result::Result<String, String>>),
    Child(String, String, bool, mpsc::SyncSender<std::result::Result<(), String>>),
    SendInput(
        String,
        Vec<Value>,
        String,
        String,
        bool,
        mpsc::SyncSender<std::result::Result<String, String>>,
    ),
    QueueCommand(
        String,
        Value,
        mpsc::SyncSender<std::result::Result<(), String>>,
    ),
    SlashCommand(
        String,
        Option<String>,
        Option<String>,
        mpsc::SyncSender<std::result::Result<(), String>>,
    ),
    SideOpen(
        Option<String>,
        mpsc::SyncSender<std::result::Result<String, String>>,
    ),
    SideSend(String, mpsc::SyncSender<std::result::Result<(), String>>),
    SideClose(mpsc::SyncSender<std::result::Result<(), String>>),
    Catalog(String, mpsc::SyncSender<Value>),
    Cancel,
    SetModel(String, mpsc::SyncSender<std::result::Result<(), String>>),
    SetMode(String, mpsc::SyncSender<std::result::Result<(), String>>),
}
/// Admitted input owns only its response receiver, so lifecycle locks can be
/// released while the native peer confirms it. Stop/handoff may interrupt wait.
pub(crate) struct PendingZCodeInput {
    response: mpsc::Receiver<std::result::Result<String, String>>,
    uncertain_message: &'static str,
}
impl PendingZCodeInput {
    pub(crate) fn wait(self) -> Result<String> {
        self.response.recv_timeout(Duration::from_secs(65))
            .context(self.uncertain_message)?.map_err(anyhow::Error::msg)
    }
}

struct Live {
    lifecycle: Mutex<()>,
    view: Mutex<View>,
    stopped: AtomicBool,
    process_id: AtomicU32,
    cancel: AtomicBool,
    sender: Mutex<Option<mpsc::Sender<Action>>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
}
impl Live {
    fn new() -> Self {
        Self {
            lifecycle: Mutex::new(()),
            view: Mutex::new(View::default()),
            stopped: AtomicBool::new(false),
            process_id: AtomicU32::new(0),
            cancel: AtomicBool::new(false),
            sender: Mutex::new(None),
            worker: Mutex::new(None),
        }
    }
}
#[derive(Clone)]
pub struct NativeZCodeSessions {
    path: PathBuf,
    store: Arc<Mutex<Store>>,
    live: Arc<Mutex<HashMap<String, Arc<Live>>>>,
    #[cfg(test)]
    pub(crate) test_peer: Option<(PathBuf, String)>,
}
fn native_terminal_outcomes(connection: &DesktopConnection<'_>) -> Vec<(String, SessionStatus)> {
    connection.rows().into_iter().filter_map(|row| {
        if row["kind"] != "turnHeader" { return None; }
        let status = match row["state"].as_str()? {
            "completedSuccess" => SessionStatus::Completed,
            "completedInterrupted" => SessionStatus::Stopped,
            "failed" => SessionStatus::Error,
            _ => return None,
        };
        let command = row["sourceCommandId"].as_str().filter(|value| !value.is_empty())?;
        Some((command.to_string(), status))
    }).collect()
}

fn record_native_terminal_outcomes(
    sessions: &SessionManager, id: &str, connection: &DesktopConnection<'_>,
    observed: &mut std::collections::HashSet<String>,
) -> Result<()> {
    for (command, outcome) in native_terminal_outcomes(connection) {
        if !observed.contains(&command) {
            sessions.record_native_turn_outcome(id, &command, outcome)?;
            observed.insert(command);
        }
    }
    Ok(())
}

impl NativeZCodeSessions {
    pub fn open_at(path: PathBuf) -> Result<Self> {
        let store: Store = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).context("解析 ZCode Desktop 会话定位失败")?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Store {
                version: 1,
                ..Default::default()
            },
            Err(e) => return Err(e.into()),
        };
        if store.version != 1 {
            bail!("不支持的 ZCode Desktop 历史版本");
        }
        Ok(Self {
            path,
            store: Arc::new(Mutex::new(store)),
            live: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(test)]
            test_peer: None,
        })
    }
    fn mutate<T>(&self, mutate: impl FnOnce(&mut Store) -> Result<T>) -> Result<T> {
        let mut store = self.store.lock().map_err(|_| anyhow!("ZCode Desktop 存储锁损坏"))?;
        let mut next = store.clone();
        let result = mutate(&mut next)?;
        save(&self.path, &next)?;
        *store = next;
        Ok(result)
    }
    pub fn register(&self, id: &str, cwd: &str) -> Result<()> {
        uuid::Uuid::parse_str(id)?;
        let cwd = crate::acp::canonical_harness_directory(cwd)?;
        self.mutate(|store| {
            if store.sessions.contains_key(id) {
                bail!("ZCode Desktop 会话已存在");
            }
            store.sessions.insert(
                id.into(),
                Locator {
                    cwd,
                    native_session_id: None,
                    rows: vec![],
                    model_selection: None,
                    permission_mode: None,
                    queued_inputs: vec![],
                    recoverable_inputs: vec![],
                },
            );
            Ok(())
        })
    }
    pub(crate) fn contains(&self, id: &str) -> bool { self.store.lock().is_ok_and(|store| store.sessions.contains_key(id)) }
    pub(crate) fn prepare_handoff(&self, id: &str) -> Result<()> {
        self.mutate(|store| { store.sessions.get_mut(id).context("接手定位不存在")?.permission_mode = Some("build".into()); Ok(()) })
    }
    pub(crate) fn send_handoff(&self, id: &str, command_id: &str, text: &str) -> Result<String> {
        uuid::Uuid::parse_str(command_id)?;
        self.submit_input_with_id(id, text, &[], false, Some(command_id))
    }
    fn locator(&self, id: &str) -> Result<Locator> {
        uuid::Uuid::parse_str(id)?;
        self.store
            .lock()
            .map_err(|_| anyhow!("ZCode Desktop 存储锁损坏"))?
            .sessions
            .get(id)
            .cloned()
            .context("未找到 ZCode Desktop 会话定位")
    }
    fn live(&self, id: &str) -> Result<Arc<Live>> {
        self.locator(id)?;
        Ok(self
            .live
            .lock()
            .map_err(|_| anyhow!("ZCode Desktop 会话表锁损坏"))?
            .entry(id.into())
            .or_insert_with(|| Arc::new(Live::new()))
            .clone())
    }
    pub fn snapshot(&self, id: &str) -> Result<ZCodeSnapshot> {
        let live = self.live(id)?;
        let view = live.view.lock().map_err(|_| anyhow!("ZCode Desktop 视图锁损坏"))?;
        let locator = self.locator(id)?;
        Ok(ZCodeSnapshot {
            session_id: id.into(),
            connected: view.connected,
            busy: view.busy,
            rows: locator.rows,
            pending_permission: view.pending.as_ref().map(|(p, _)| p.clone()),
            sequence: view.sequence,
            error: view.error.clone(),
            models: view.models.clone(),
            selected_model_id: view.selected_model_id.clone(),
            permission_mode: view.permission_mode.clone().or(locator.permission_mode),
            queued_inputs: view.queued_inputs.clone(),
            recoverable_inputs: locator.recoverable_inputs,
            queue_auto_drain: view.queue_auto_drain,
            can_queue: view.can_queue,
            input_routing: view.input_routing.clone(),
            child_detail: view.child_detail.clone(),
            side_detail: view.side_detail.clone(),
            context_usage: view.context_usage.clone(),
        })
    }
    fn publish<R: tauri::Runtime>(
        &self,
        app: &AppHandle<R>,
        id: &str,
        live: &Live,
        record: Option<SessionRecord>,
    ) -> Result<()> {
        let mut view = live.view.lock().map_err(|_| anyhow!("ZCode Desktop 视图锁损坏"))?;
        view.sequence += 1;
        let locator = self.locator(id)?;
        let rows = locator.rows;
        let snapshot = ZCodeSnapshot {
            session_id: id.into(),
            connected: view.connected,
            busy: view.busy,
            rows,
            pending_permission: view.pending.as_ref().map(|(p, _)| p.clone()),
            sequence: view.sequence,
            error: view.error.clone(),
            models: view.models.clone(),
            selected_model_id: view.selected_model_id.clone(),
            permission_mode: view.permission_mode.clone(),
            queued_inputs: view.queued_inputs.clone(),
            recoverable_inputs: locator.recoverable_inputs,
            queue_auto_drain: view.queue_auto_drain,
            can_queue: view.can_queue,
            input_routing: view.input_routing.clone(),
            child_detail: view.child_detail.clone(),
            side_detail: view.side_detail.clone(),
            context_usage: view.context_usage.clone(),
        };
        app.emit(
            "zcode-session-update",
            json!({"snapshot":snapshot,"record":record}),
        )?;
        Ok(())
    }
    fn sync_queue_state(
        &self,
        id: &str,
        live: &Live,
        connection: &DesktopConnection<'_>,
        reconnect: bool,
        awaiting_foreground: bool,
    ) -> Result<()> {
        let queue = connection.queue();
        self.mutate(|store| {
            let locator = store.sessions.get_mut(id).context("ZCode Desktop 会话已删除")?;
            if reconnect {
                for item in &locator.queued_inputs {
                    let command = &item["sourceCommandId"];
                    let exists = queue
                        .iter()
                        .any(|q| q["queueItemId"] == item["queueItemId"]);
                    let executed = connection.rows().iter().any(|row| {
                        row["kind"] == "turnHeader" && &row["sourceCommandId"] == command
                    });
                    if !exists
                        && !executed
                        && !locator
                            .recoverable_inputs
                            .iter()
                            .any(|q| q["queueItemId"] == item["queueItemId"])
                    {
                        locator.recoverable_inputs.push(item.clone());
                    }
                }
            }
            locator.queued_inputs = queue.clone();
            locator.recoverable_inputs.retain(|item| {
                !queue
                    .iter()
                    .any(|q| q["sourceCommandId"] == item["sourceCommandId"])
                    && !connection.rows().iter().any(|row| {
                        row["kind"] == "turnHeader"
                            && row["sourceCommandId"] == item["sourceCommandId"]
                    })
            });
            locator.rows = connection.rows();
            locator.permission_mode = Some(connection.permission_mode.clone());
            Ok(())
        })?;
        let mut view = live.view.lock().unwrap();
        view.queued_inputs = queue;
        view.queue_auto_drain = connection.queue_auto_drain();
        view.can_queue = connection.can_queue();
        view.input_routing = connection.input_routing();
        if let Some(busy) = connection.native_busy() {
            view.busy = busy || awaiting_foreground;
        }
        Ok(())
    }
    pub fn start<R: tauri::Runtime>(
        &self,
        app: AppHandle<R>,
        sessions: &SessionManager,
        record: &SessionRecord,
    ) -> Result<ZCodeSnapshot> {
        let _admission = app.try_state::<crate::runtime_admission::RuntimeAdmission>()
            .map(|admission| admission.admit()).transpose()?;
        if !is_standalone_zcode(record) || record.archived_at.is_some() {
            bail!("会话不是可启动的独立 ZCode Desktop 会话");
        }
        let live = self.live(&record.id)?;
        let _lifecycle = live
            .lifecycle
            .lock()
            .map_err(|_| anyhow!("ZCode Desktop 生命周期锁损坏"))?;
        if live.view.lock().unwrap().connected {
            return self.snapshot(&record.id);
        }
        if let Some(worker) = live.worker.lock().unwrap().take() {
            worker.join().map_err(|_| anyhow!("ZCode Desktop 工作线程异常"))?;
        }
        // A disconnected worker can still own a Job whose checked stop failed.
        // Never overwrite that identity with a replacement process.
        crate::acp::terminate_acp_process_checked(&live.process_id)?;
        let locator = self.locator(&record.id)?;
        if locator.cwd != crate::acp::canonical_harness_directory(&record.cwd)? {
            bail!("ZCode Desktop 恢复目录与原会话不一致");
        }
        if sessions
            .get(&record.id)
            .is_none_or(|r| r.archived_at.is_some())
        {
            bail!("归档或已删除会话不能启动");
        }
        // zcode 与 zcode_cli 两个 Runtime 共用此原生会话栈，按会话记录选择入口。
        let runtime_kind = match record.agent_runtime_kind.as_deref() {
            Some("zcode_cli") => AgentRuntimeKind::ZCodeCli,
            _ => AgentRuntimeKind::ZCode,
        };
        let lease = crate::runtime_preferences::acquire(runtime_kind)?;
        #[cfg(not(test))]
        let command = zcode::launch(&zcode::runtime_executable(runtime_kind)?)?;
        #[cfg(test)]
        let command = if let Some((path, scenario)) = &self.test_peer {
            let mut c = std::process::Command::new(which::which("node")?);
            c.arg(path).env("ZCODE_NATIVE_SCENARIO", scenario);
            c
        } else {
            zcode::launch(&zcode::runtime_executable(runtime_kind)?)?
        };
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        *live.sender.lock().unwrap() = Some(tx);
        live.stopped.store(false, Ordering::Release);
        live.cancel.store(false, Ordering::Release);
        {
            let mut view = live.view.lock().unwrap();
            view.error = None;
            view.pending = None;
            view.busy = false;
        }
        let manager = self.clone();
        let event_app = app.clone();
        let session_manager = sessions.clone();
        let id = record.id.clone();
        let worker_live = live.clone();
        let worker = thread::spawn(move || {
            let mut startup_admission = _admission;
            let _lease = lease;
            let mut permission = |params: &Value, _: &AtomicBool| -> Result<bool> {
                if worker_live.stopped.load(Ordering::Acquire)
                    || worker_live.cancel.load(Ordering::Acquire)
                {
                    return Ok(false);
                }
                let (tx, rx) = mpsc::sync_channel(1);
                worker_live.view.lock().unwrap().pending = Some((params.clone(), tx));
                let record = session_manager.mark_native_runtime_status(
                    &id,
                    SessionStatus::Waiting,
                    Some("需要确认 ZCode Desktop 工具调用".into()),
                    None,
                )?;
                manager.publish(&event_app, &id, &worker_live, Some(record))?;
                let deadline = Instant::now() + Duration::from_secs(1800);
                let decision = loop {
                    if worker_live.stopped.load(Ordering::Acquire)
                        || worker_live.cancel.load(Ordering::Acquire)
                        || Instant::now() > deadline
                    {
                        break false;
                    }
                    match rx.recv_timeout(Duration::from_millis(50)) {
                        Ok(value) => break value,
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(_) => break false,
                    }
                };
                worker_live.view.lock().unwrap().pending = None;
                if !worker_live.stopped.load(Ordering::Acquire) {
                    let record = session_manager.mark_native_runtime_status(
                        &id,
                        SessionStatus::Running,
                        None,
                        None,
                    )?;
                    manager.publish(&event_app, &id, &worker_live, Some(record))?;
                }
                Ok(decision)
            };
            let mut progress = || Ok(());
            let outcome = (|| -> Result<()> {
                // 与其它原生聊天一致：注入启用的 stdio MCP（浏览器/电脑控制），
                // 启动时只刷新桥接，实际浏览器工具调用才激活面板。
                let mcp_servers = crate::multi_agent::zcode::zcode_desktop_mcp_servers();
                if mcp_servers
                    .iter()
                    .any(|server| server.get("name").and_then(Value::as_str) == Some("browser-control"))
                {
                    crate::browser_control::refresh_bridge();
                }
                let mut connection = DesktopConnection::open_tracked(
                    command,
                    Path::new(&locator.cwd),
                    locator.native_session_id.as_deref(),
                    locator.rows.is_empty(),
                    mcp_servers,
                    &worker_live.stopped,
                    &mut permission,
                    &mut progress,
                    Some(&worker_live.process_id),
                )?;
                if let Some(mode) = locator.permission_mode.as_deref() {
                    if mode != connection.permission_mode {
                        connection.set_permission_mode(
                            mode,
                            &worker_live.stopped,
                            &mut permission,
                        )?;
                    }
                }
                if connection.selected_model_id.is_none() {
                    if let Some(selection) = locator.model_selection.as_ref() {
                        let key = selection.to_string();
                        if connection.models.iter().any(|model| model.id == key) {
                            connection.set_model(&key, &worker_live.stopped, &mut permission)?;
                        }
                    }
                }
                // Invalid or removed native selections remain unselected; never
                // substitute another model or enable account authentication.
                let selected = connection
                    .selected_model_id
                    .clone()
                    .filter(|key| connection.models.iter().any(|model| &model.id == key));
                {
                    let mut view = worker_live.view.lock().unwrap();
                    view.permission_mode = Some(connection.permission_mode.clone());
                    view.models = connection.models.clone();
                    view.selected_model_id = selected.clone();
                    view.error = connection.model_catalog_notice();
                }
                manager.mutate(|store| {
                    let entry = store
                        .sessions
                        .get_mut(&id)
                        .context("ZCode Desktop 会话定位已删除")?;
                    if !entry.rows.is_empty()
                        && entry
                            .native_session_id
                            .as_ref()
                            .is_some_and(|s| s != &connection.session_id)
                    {
                        bail!("ZCode Desktop 恢复标识改变");
                    }
                    entry.permission_mode = Some(connection.permission_mode.clone());
                    entry.native_session_id = Some(connection.session_id.clone());
                    entry.rows = connection.rows();
                    entry.model_selection = selected
                        .as_ref()
                        .and_then(|key| serde_json::from_str(key).ok());
                    Ok(())
                })?;
                if worker_live.stopped.load(Ordering::Acquire) {
                    bail!("ZCode Desktop 启动已取消");
                }
                manager.sync_queue_state(&id, &worker_live, &connection, true, false)?;
                worker_live.view.lock().unwrap().connected = true;
                let mut record = session_manager.mark_native_runtime_status(
                    &id,
                    SessionStatus::Running,
                    None,
                    None,
                )?;
                let mut observed_title = None;
                if let Some(updated) = sync_session_title(
                    &session_manager, &id, connection.session_title(), &mut observed_title,
                ) {
                    record = updated;
                }
                manager.publish(&event_app, &id, &worker_live, Some(record))?;
                drop(startup_admission.take());
                let _ = ready_tx.send(Ok(()));
                let mut active: Option<String> = None;
                // Seed exact old terminal IDs so resumed history never replays automations.
                let mut observed_outcomes = native_terminal_outcomes(&connection).into_iter().map(|(id, _)| id).collect::<std::collections::HashSet<_>>();
                let mut cancelled_at: Option<Instant> = None;
                loop {
                    if worker_live.stopped.load(Ordering::Acquire) {
                        connection.shutdown()?;
                        return Ok(());
                    }
                    // Commands may poll internally, so also inspect the projection when
                    // the outer poll has no new frame. Child/side projections are excluded.
                    if let Some(record) = sync_session_title(
                        &session_manager, &id, connection.session_title(), &mut observed_title,
                    ) {
                        manager.publish(&event_app, &id, &worker_live, Some(record))?;
                    }
                    if let Ok(action) = rx.try_recv() {
                        match action {
                            Action::SendInput(text, attachments, delivery, command_id, plan_enabled, reply) => {
                                let result = connection.send_input_delivery_id(
                                    &text,
                                    &attachments,
                                    &delivery,
                                    &command_id,
                                    &worker_live.stopped,
                                    Some(&worker_live.cancel),
                                    plan_enabled,
                                    &mut permission,
                                );
                                match &result {
                                    Ok(command) => {
                                        if delivery == "startNow" {
                                            active = Some(command.clone());
                                        }
                                    }
                                    Err(error)
                                        if error
                                            .downcast_ref::<zcode::CommandRejected>()
                                            .is_some()
                                            || error
                                                .downcast_ref::<zcode::RequestRejected>()
                                                .is_some() =>
                                    {
                                        let mut view = worker_live.view.lock().unwrap();
                                        if delivery == "startNow" {
                                            view.busy = false;
                                        }
                                        view.error = Some(error.to_string());
                                        drop(view);
                                        manager.mutate(|store| {
                                            store
                                                .sessions
                                                .get_mut(&id)
                                                .context("ZCode Desktop 会话已删除")?
                                                .recoverable_inputs
                                                .retain(|item| {
                                                    item["sourceCommandId"] != command_id
                                                });
                                            Ok(())
                                        })?;
                                    }
                                    Err(_) => {}
                                }
                                if result.is_ok() {
                                    if active.as_deref().is_some_and(|command| {
                                        matches!(
                                            connection.turn_state(command),
                                            Some(
                                                "completedSuccess"
                                                    | "completedInterrupted"
                                                    | "failed"
                                            )
                                        )
                                    }) {
                                        active = None;
                                        worker_live.view.lock().unwrap().busy = false;
                                    }
                                    manager.sync_queue_state(
                                        &id,
                                        &worker_live,
                                        &connection,
                                        false,
                                        active.as_deref().is_some_and(|command| {
                                            !matches!(
                                                connection.turn_state(command),
                                                Some(
                                                    "completedSuccess"
                                                        | "completedInterrupted"
                                                        | "failed"
                                                )
                                            )
                                        }),
                                    )?;
                                    manager.mutate(|store| {
                                        store
                                            .sessions
                                            .get_mut(&id)
                                            .context("ZCode Desktop 会话已删除")?
                                            .rows = connection.rows();
                                        Ok(())
                                    })?;
                                }
                                record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                                manager.publish(&event_app, &id, &worker_live, None)?;
                                let _ = reply.send(result.as_ref().map(Clone::clone).map_err(
                                    |error| {
                                        let rejected = error
                                            .downcast_ref::<zcode::CommandRejected>()
                                            .is_some()
                                            || error
                                                .downcast_ref::<zcode::RequestRejected>()
                                                .is_some();
                                        format!(
                                            "{}: {}",
                                            if rejected {
                                                "ZCODE_INPUT_REJECTED"
                                            } else {
                                                "ZCODE_INPUT_UNCERTAIN"
                                            },
                                            error
                                        )
                                    },
                                ));
                                if let Err(error) = result {
                                    if (error.downcast_ref::<zcode::CommandRejected>().is_none()
                                        && error.downcast_ref::<zcode::RequestRejected>().is_none())
                                    {
                                        return Err(error);
                                    }
                                }
                                cancelled_at = None;
                            }
                            Action::QueueCommand(kind, payload, reply) => {
                                let result = connection.queue_command(
                                    &kind,
                                    payload,
                                    &worker_live.stopped,
                                    &mut permission,
                                );
                                if result.is_ok() {
                                    manager.sync_queue_state(
                                        &id,
                                        &worker_live,
                                        &connection,
                                        false,
                                        active.as_deref().is_some_and(|command| {
                                            !matches!(
                                                connection.turn_state(command),
                                                Some(
                                                    "completedSuccess"
                                                        | "completedInterrupted"
                                                        | "failed"
                                                )
                                            )
                                        }),
                                    )?;
                                    record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                                    manager.publish(&event_app, &id, &worker_live, None)?;
                                }
                                let _ = reply.send(
                                    result
                                        .as_ref()
                                        .map(|_| ())
                                        .map_err(|error| error.to_string()),
                                );
                                if let Err(error) = result {
                                    if (error.downcast_ref::<zcode::CommandRejected>().is_none()
                                        && error.downcast_ref::<zcode::RequestRejected>().is_none())
                                    {
                                        return Err(error);
                                    }
                                }
                            }
                            Action::Edit(row_id, entity_id, text, reply) => {
                                let result = connection.edit_user_query(row_id, &entity_id, &text, &worker_live.stopped, &mut permission);
                                if let Ok(command) = &result {
                                    active = Some(command.clone());
                                    worker_live.view.lock().unwrap().busy = true;
                                    worker_live.cancel.store(false, Ordering::Release);
                                    record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                                    manager.publish(&event_app, &id, &worker_live, None)?;
                                }
                                let _ = reply.send(result.as_ref().cloned().map_err(|error| error.to_string()));
                                if let Err(error) = result {
                                    if error.downcast_ref::<zcode::CommandRejected>().is_none() && error.downcast_ref::<zcode::RequestRejected>().is_none() { return Err(error); }
                                }
                            }
                            Action::SlashCommand(kind, text, display_text, reply) => {
                                let result = connection.slash_command(
                                    &kind,
                                    text.as_deref(),
                                    display_text.as_deref(),
                                    &worker_live.stopped,
                                    &mut permission,
                                );
                                if result.is_ok() {
                                    manager.sync_queue_state(
                                        &id,
                                        &worker_live,
                                        &connection,
                                        false,
                                        active.as_deref().is_some_and(|command| {
                                            !matches!(
                                                connection.turn_state(command),
                                                Some(
                                                    "completedSuccess"
                                                        | "completedInterrupted"
                                                        | "failed"
                                                )
                                            )
                                        }),
                                    )?;
                                    record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                                    manager.publish(&event_app, &id, &worker_live, None)?;
                                }
                                let _ = reply.send(
                                    result
                                        .as_ref()
                                        .map(|_| ())
                                        .map_err(|error| error.to_string()),
                                );
                                if let Err(error) = result {
                                    if (error.downcast_ref::<zcode::CommandRejected>().is_none()
                                        && error.downcast_ref::<zcode::RequestRejected>().is_none())
                                    {
                                        return Err(error);
                                    }
                                }
                            }
                            Action::Child(child, owner, open, reply) => {
                                let result = if open { connection.open_child(&child, &owner, &worker_live.stopped, &mut permission) } else { connection.close_child(Some(&child), Some(&owner), &worker_live.stopped, &mut permission) };
                                worker_live.view.lock().unwrap().child_detail = connection.child_detail();
                                worker_live.view.lock().unwrap().side_detail = connection.side_detail();
                                let _ = reply.send(result.map_err(|error| error.to_string()));
                                record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                                manager.publish(&event_app, &id, &worker_live, None)?;
                            }
                            Action::SideOpen(first_input, reply) => {
                                let result = connection.create_side_session(first_input.as_deref(), &worker_live.stopped, &mut permission);
                                worker_live.view.lock().unwrap().side_detail = connection.side_detail();
                                let _ = reply.send(result.as_ref().map(Clone::clone).map_err(|error| error.to_string()));
                                record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                                manager.publish(&event_app, &id, &worker_live, None)?;
                                if let Err(error) = result {
                                    if (error.downcast_ref::<zcode::CommandRejected>().is_none()
                                        && error.downcast_ref::<zcode::RequestRejected>().is_none())
                                    {
                                        return Err(error);
                                    }
                                }
                            }
                            Action::SideSend(text, reply) => {
                                let result = connection.side_send(&text, &worker_live.stopped, &mut permission);
                                worker_live.view.lock().unwrap().side_detail = connection.side_detail();
                                let _ = reply.send(result.as_ref().map(|_| ()).map_err(|error| error.to_string()));
                                record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                                manager.publish(&event_app, &id, &worker_live, None)?;
                                if let Err(error) = result {
                                    if (error.downcast_ref::<zcode::CommandRejected>().is_none()
                                        && error.downcast_ref::<zcode::RequestRejected>().is_none())
                                    {
                                        return Err(error);
                                    }
                                }
                            }
                            Action::SideClose(reply) => {
                                let result = connection.close_side(&worker_live.stopped, &mut permission);
                                worker_live.view.lock().unwrap().side_detail = None;
                                let _ = reply.send(result.map_err(|error| error.to_string()));
                                record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                                manager.publish(&event_app, &id, &worker_live, None)?;
                            }
                            Action::Catalog(cwd, reply) => {
                                let catalog = connection.composer_catalog(
                                    &cwd,
                                    &worker_live.stopped,
                                    &mut permission,
                                );
                                let _ = reply.send(catalog);
                            }
                            Action::SetMode(mode, reply) => {
                                let result = (|| -> Result<()> {
                                    connection.set_permission_mode(
                                        &mode,
                                        &worker_live.stopped,
                                        &mut permission,
                                    )?;
                                    manager.mutate(|store| {
                                        store
                                            .sessions
                                            .get_mut(&id)
                                            .context("ZCode Desktop 会话已删除")?
                                            .permission_mode =
                                            Some(connection.permission_mode.clone());
                                        Ok(())
                                    })?;
                                    let mut view = worker_live.view.lock().unwrap();
                                    view.permission_mode = Some(connection.permission_mode.clone());
                                    view.error = None;
                                    drop(view);
                                    record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                                    manager.publish(&event_app, &id, &worker_live, None)?;
                                    Ok(())
                                })();
                                let _ = reply.send(
                                    result
                                        .as_ref()
                                        .map(|_| ())
                                        .map_err(|error| error.to_string()),
                                );
                                // Native rejections keep the connection usable; persistence errors stop it.
                                if let Err(error) = result {
                                    if error.downcast_ref::<zcode::RequestRejected>().is_none() {
                                        return Err(error);
                                    }
                                }
                            }
                            Action::SetModel(model_id, reply) => {
                                let result = (|| -> Result<()> {
                                    let selection = connection.set_model(
                                        &model_id,
                                        &worker_live.stopped,
                                        &mut permission,
                                    )?;
                                    manager.mutate(|store| {
                                        store
                                            .sessions
                                            .get_mut(&id)
                                            .context("ZCode Desktop 会话已删除")?
                                            .model_selection = Some(selection);
                                        Ok(())
                                    })?;
                                    let mut view = worker_live.view.lock().unwrap();
                                    view.selected_model_id = connection.selected_model_id.clone();
                                    view.error = None;
                                    drop(view);
                                    record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                                    manager.publish(&event_app, &id, &worker_live, None)?;
                                    Ok(())
                                })();
                                let _ = reply.send(
                                    result
                                        .as_ref()
                                        .map(|_| ())
                                        .map_err(|error| error.to_string()),
                                );
                                // A native request rejection leaves the previous model
                                // authoritative. Transport/persistence failures disconnect.
                                if let Err(error) = result {
                                    if error.downcast_ref::<zcode::RequestRejected>().is_none() {
                                        return Err(error);
                                    }
                                }
                            }
                            Action::Cancel => {
                                if active.is_some() || connection.native_busy() == Some(true) {
                                    connection.cancel(&worker_live.stopped, &mut permission)?;
                                    cancelled_at = Some(Instant::now());
                                } else {
                                    worker_live.cancel.store(false, Ordering::Release);
                                }
                            }
                        }
                    }
                    if connection.poll(&worker_live.stopped, &mut permission)? {
                        worker_live.view.lock().unwrap().child_detail = connection.child_detail();
                        worker_live.view.lock().unwrap().side_detail = connection.side_detail();
                        if let Some(error) = connection.take_auth_error() {
                            worker_live.view.lock().unwrap().error = Some(error);
                        }
                        {
                            let mut view = worker_live.view.lock().unwrap();
                            view.permission_mode = Some(connection.permission_mode.clone());
                            view.context_usage = connection.context_usage();
                        }
                        manager.mutate(|store| {
                            store
                                .sessions
                                .get_mut(&id)
                                .context("ZCode Desktop 会话已删除")?
                                .permission_mode = Some(connection.permission_mode.clone());
                            store
                                .sessions
                                .get_mut(&id)
                                .context("ZCode Desktop 会话已删除")?
                                .rows = connection.rows();
                            Ok(())
                        })?;
                        if let Some(command) = active.as_deref() {
                            match connection.turn_state(command) {
                                Some("completedSuccess" | "completedInterrupted" | "failed") => {
                                    active = None;
                                    worker_live.view.lock().unwrap().busy = false;
                                    worker_live.cancel.store(false, Ordering::Release);
                                    cancelled_at = None;
                                }
                                Some("running") | None => {}
                                Some(_) => bail!("ZCode Desktop 当前任务状态未知"),
                            }
                        }
                        manager.sync_queue_state(
                            &id,
                            &worker_live,
                            &connection,
                            false,
                            active.as_deref().is_some_and(|command| {
                                !matches!(
                                    connection.turn_state(command),
                                    Some("completedSuccess" | "completedInterrupted" | "failed")
                                )
                            }),
                        )?;
                        if connection.native_busy() == Some(false) {
                            cancelled_at = None;
                        }
                        record_native_terminal_outcomes(&session_manager, &id, &connection, &mut observed_outcomes)?;
                        manager.publish(&event_app, &id, &worker_live, None)?;
                    }
                    if cancelled_at.is_some_and(|at| at.elapsed() > Duration::from_secs(10)) {
                        bail!("ZCode Desktop 未确认取消；连接已关闭，部分内容保留");
                    }
                }
            })();
            let error = outcome
                .err()
                .filter(|_| !worker_live.stopped.load(Ordering::Acquire))
                .map(|e| e.to_string());
            {
                let mut view = worker_live.view.lock().unwrap();
                view.connected = false;
                view.busy = false;
                view.pending = None;
                view.child_detail = None;
                view.side_detail = None;
                view.error = error.clone();
            }
            let _ = ready_tx.try_send(Err(error
                .clone()
                .unwrap_or_else(|| "ZCode Desktop 会话已停止".into())));
            if !worker_live.stopped.load(Ordering::Acquire) {
                if let Ok(record) = session_manager.mark_native_runtime_status(
                    &id,
                    SessionStatus::Error,
                    None,
                    error,
                ) {
                    let _ = manager.publish(&event_app, &id, &worker_live, Some(record));
                }
            }
        });
        *live.worker.lock().unwrap() = Some(worker);
        match ready_rx.recv_timeout(Duration::from_secs(65)) {
            Ok(Ok(())) => self.snapshot(&record.id),
            Ok(Err(e)) => Err(anyhow!(e)),
            Err(_) => {
                live.stopped.store(true, Ordering::Release);
                Err(anyhow!("ZCode Desktop 启动超时"))
            }
        }
    }
    pub fn send(&self, id: &str, text: &str) -> Result<()> {
        self.send_input(id, text, &[], false).map(|_| ())
    }
    pub fn send_input(&self, id: &str, text: &str, paths: &[String], plan_enabled: bool) -> Result<String> {
        self.admit_input(id, text, paths, plan_enabled)?.wait()
    }
    pub(crate) fn admit_input(&self, id: &str, text: &str, paths: &[String], plan_enabled: bool) -> Result<PendingZCodeInput> {
        self.admit_input_with_id(id, text, paths, plan_enabled, None).map_err(|error| {
            if error.to_string().starts_with("ZCODE_INPUT_") {
                error
            } else {
                anyhow!("ZCODE_INPUT_REJECTED: {error}")
            }
        })
    }
    fn submit_input_with_id(&self, id: &str, text: &str, paths: &[String], plan_enabled: bool, handoff_id: Option<&str>) -> Result<String> {
        self.admit_input_with_id(id, text, paths, plan_enabled, handoff_id)?.wait()
    }
    fn admit_input_with_id(&self, id: &str, text: &str, paths: &[String], plan_enabled: bool, handoff_id: Option<&str>) -> Result<PendingZCodeInput> {
        let text = text.trim();
        let attachments = prepare_attachments(paths)?;
        if (text.is_empty() && attachments.is_empty()) || text.chars().count() > 32000 {
            bail!("消息不能为空且最多 32000 字符");
        }
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        let mut view = live.view.lock().unwrap();
        if !view.connected || live.stopped.load(Ordering::Acquire) {
            bail!("ZCode Desktop 未连接，请恢复会话后重试");
        }
        if view.pending.is_some() {
            bail!("请先处理当前工具审批，再发送下一条消息");
        }
        if view.selected_model_id.is_none() {
            bail!("请先选择 ZCode Desktop 模型");
        }
        let delivery = if view.busy {
            if !view.can_queue
                || !matches!(view.input_routing.as_deref(), Some("enqueue" | "guide"))
            {
                bail!("ZCode Desktop 当前不接受排队输入，请稍后重试");
            }
            "queue"
        } else {
            if view.can_queue && view.input_routing.as_deref() != Some("startNow") {
                bail!("请先继续或撤销暂停的队列，再发送新消息");
            }
            "startNow"
        };
        let was_busy = view.busy;
        if delivery == "startNow" {
            live.cancel.store(false, Ordering::Release);
            view.busy = true;
        }
        view.error = None;
        drop(view);
        let command_id = handoff_id.map(str::to_owned).unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        self.mutate(|store| {
            store.sessions.get_mut(id).context("ZCode Desktop 会话已删除")?.recoverable_inputs.push(json!({"queueItemId":command_id,"sourceCommandId":command_id,"text":text,"attachments":attachments,"dispatch":{"state":"queued"}}));
            Ok(())
        }).map_err(|error| {
            live.view.lock().unwrap().busy = was_busy;
            anyhow!("ZCODE_INPUT_REJECTED: 输入未发送，无法保存待核对内容：{error}")
        })?;
        let (tx, rx) = mpsc::sync_channel(1);
        let sender = live.sender.lock().unwrap().clone();
        if sender.is_none()
            || sender
                .unwrap()
                .send(Action::SendInput(
                    text.into(),
                    attachments,
                    delivery.into(),
                    command_id.clone(),
                    plan_enabled,
                    tx,
                ))
                .is_err()
        {
            live.view.lock().unwrap().busy = was_busy;
            self.mutate(|store| {
                store
                    .sessions
                    .get_mut(id)
                    .context("ZCode Desktop 会话已删除")?
                    .recoverable_inputs
                    .retain(|item| item["sourceCommandId"] != command_id);
                Ok(())
            })?;
            bail!("ZCode Desktop 连接已关闭");
        }
        // Native ACK identifies this exact submission; unrelated user rows cannot acknowledge it.
        drop(_life);
        Ok(PendingZCodeInput { response: rx, uncertain_message: "ZCODE_INPUT_UNCERTAIN: ZCode Desktop 输入确认等待超时，请先核对历史" })
    }
    pub fn queue_action(&self, id: &str, item_id: Option<&str>) -> Result<ZCodeSnapshot> {
        self.queue_action_op(id, item_id, item_id.map(|_| "delete"), None)
    }
    /// 队列条目操作。op: None=继续队列(恢复 autoDrain)；Some("delete")=撤销；
    /// Some("send_now")=立即插队发送；Some("edit")=改写条目文本。
    pub fn queue_action_op(&self, id: &str, item_id: Option<&str>, op: Option<&str>, new_text: Option<&str>) -> Result<ZCodeSnapshot> {
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        let view = live.view.lock().unwrap();
        if !view.connected
            || view.pending.is_some()
            || !view.can_queue
            || live.stopped.load(Ordering::Acquire)
        {
            bail!("ZCode Desktop 队列当前不可操作");
        }
        let needs_item = op != Some("drain");
        let (kind, payload) = match (op, item_id) {
            (Some("send_now"), Some(item_id)) => {
                let item = view.queued_inputs.iter().find(|item| item["queueItemId"] == item_id).context("队列条目已被消费或撤销")?;
                if item.pointer("/dispatch/state").and_then(Value::as_str) != Some("queued") {
                    bail!("队列条目正在开始执行，不能插队");
                }
                ("sendQueuedNow", json!({"queueItemId":item_id}))
            }
            (Some("edit"), Some(item_id)) => {
                let item = view.queued_inputs.iter().find(|item| item["queueItemId"] == item_id).context("队列条目已被消费或撤销")?;
                if item.pointer("/dispatch/state").and_then(Value::as_str) != Some("queued") {
                    bail!("队列条目正在开始执行，不能编辑");
                }
                let text = new_text.map(str::trim).filter(|t| !t.is_empty()).context("编辑后的内容不能为空")?;
                ("editQueueItem", json!({"queueItemId":item_id,"newText":text}))
            }
            (Some("delete"), Some(item_id)) => {
                let item = view.queued_inputs.iter().find(|item| item["queueItemId"] == item_id).context("队列条目已被消费或撤销")?;
                if item.pointer("/dispatch/state").and_then(Value::as_str) != Some("queued") {
                    bail!("队列条目正在开始执行，不能撤销");
                }
                ("deleteQueueItem", json!({"queueItemId":item_id}))
            }
            (None, _) => ("setAutoDrain", json!({"autoDrain":true})),
            _ => bail!("ZCode Desktop 队列操作缺少条目标识"),
        };
        let _ = needs_item;
        drop(view);
        let (tx, rx) = mpsc::sync_channel(1);
        live.sender
            .lock()
            .unwrap()
            .as_ref()
            .context("ZCode Desktop 连接已关闭")?
            .send(Action::QueueCommand(kind.into(), payload, tx))
            .context("ZCode Desktop 队列操作发送失败")?;
        drop(_life);
        rx.recv_timeout(Duration::from_secs(65))
            .context("ZCode Desktop 队列操作等待超时")?
            .map_err(anyhow::Error::msg)?;
        self.snapshot(id)
    }
    /// 原生桌面 UI 层消费的可见斜杠命令（/goal /target /compact /compress /goal resume）。
    /// kind 为解析后的类型化命令（sendGoalCommand/compact/resumeGoal），busy 时由服务端排队。
    pub fn slash_command(
        &self,
        id: &str,
        kind: &str,
        text: Option<&str>,
        display_text: Option<&str>,
    ) -> Result<()> {
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        let mut view = live.view.lock().unwrap();
        if !view.connected || live.stopped.load(Ordering::Acquire) {
            bail!("ZCode Desktop 未连接，请恢复会话后重试");
        }
        if view.pending.is_some() {
            bail!("请先处理当前工具审批，再发送命令");
        }
        if view.selected_model_id.is_none() {
            bail!("请先选择 ZCode Desktop 模型");
        }
        let was_busy = view.busy;
        view.busy = true;
        view.error = None;
        drop(view);
        let (tx, rx) = mpsc::sync_channel(1);
        let send_result = live
            .sender
            .lock()
            .unwrap()
            .as_ref()
            .context("ZCode Desktop 连接已关闭")?
            .send(Action::SlashCommand(
                kind.into(),
                text.map(str::to_owned),
                display_text.map(str::to_owned),
                tx,
            ));
        if send_result.is_err() {
            live.view.lock().unwrap().busy = was_busy;
            bail!("ZCode Desktop 连接已关闭");
        }
        drop(_life);
        // recv_timeout 外层是超时，内层才是命令结果；内层 Err（如空目标、plan/goal 互斥）必须透传。
        match rx
            .recv_timeout(Duration::from_secs(65))
            .context("ZCode Desktop 命令等待超时，请重新加载会话核对")
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(message)) => {
                live.view.lock().unwrap().busy = was_busy;
                bail!(message);
            }
            Err(error) => {
                live.view.lock().unwrap().busy = was_busy;
                Err(error)
            }
        }
    }
    /// /side|/btw：在父会话同一 ZCode Desktop 进程内派生辅助会话并打开分栏（原生
    /// selection-side-session 同语义）。不影响父会话队列与运行态，busy 时同样可用。
    pub fn side_session(&self, id: &str, first_input: Option<&str>) -> Result<ZCodeSnapshot> {
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        let view = live.view.lock().unwrap();
        if !view.connected || live.stopped.load(Ordering::Acquire) {
            bail!("ZCode Desktop 未连接，请恢复会话后重试");
        }
        if view.pending.is_some() {
            bail!("请先处理当前工具审批，再打开辅助对话");
        }
        if view.selected_model_id.is_none() {
            bail!("请先选择 ZCode Desktop 模型");
        }
        drop(view);
        let (tx, rx) = mpsc::sync_channel(1);
        live.sender
            .lock()
            .unwrap()
            .as_ref()
            .context("ZCode Desktop 连接已关闭")?
            .send(Action::SideOpen(first_input.map(str::to_owned), tx))
            .context("辅助对话请求发送失败")?;
        drop(_life);
        rx.recv_timeout(Duration::from_secs(65))
            .context("辅助对话创建等待超时")?
            .map_err(anyhow::Error::msg)?;
        self.snapshot(id)
    }
    pub fn side_send_message(&self, id: &str, text: &str) -> Result<ZCodeSnapshot> {
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        let view = live.view.lock().unwrap();
        if !view.connected || live.stopped.load(Ordering::Acquire) {
            bail!("ZCode Desktop 未连接，请恢复会话后重试");
        }
        if view.pending.is_some() {
            bail!("请先处理当前工具审批，再发送辅助对话消息");
        }
        if view.side_detail.is_none() {
            bail!("辅助对话未打开");
        }
        drop(view);
        let (tx, rx) = mpsc::sync_channel(1);
        live.sender
            .lock()
            .unwrap()
            .as_ref()
            .context("ZCode Desktop 连接已关闭")?
            .send(Action::SideSend(text.trim().to_owned(), tx))
            .context("辅助对话消息发送失败")?;
        drop(_life);
        rx.recv_timeout(Duration::from_secs(65))
            .context("辅助对话消息等待超时")?
            .map_err(anyhow::Error::msg)?;
        self.snapshot(id)
    }
    pub fn side_close(&self, id: &str) -> Result<ZCodeSnapshot> {
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        let view = live.view.lock().unwrap();
        if !view.connected || live.stopped.load(Ordering::Acquire) {
            bail!("ZCode Desktop 未连接，请恢复会话后重试");
        }
        drop(view);
        let (tx, rx) = mpsc::sync_channel(1);
        live.sender
            .lock()
            .unwrap()
            .as_ref()
            .context("ZCode Desktop 连接已关闭")?
            .send(Action::SideClose(tx))
            .context("辅助对话关闭请求发送失败")?;
        drop(_life);
        rx.recv_timeout(Duration::from_secs(65))
            .context("辅助对话关闭等待超时")?
            .map_err(anyhow::Error::msg)?;
        self.snapshot(id)
    }
    pub fn recover_queue_input(&self, id: &str, item_id: &str, discard: bool) -> Result<Value> {
        self.mutate(|store| {
            let locator = store.sessions.get_mut(id).context("ZCode Desktop 会话已删除")?;
            let index = locator
                .recoverable_inputs
                .iter()
                .position(|item| item["queueItemId"] == item_id)
                .context("未发送内容已恢复")?;
            Ok(if discard {
                locator.recoverable_inputs.remove(index)
            } else {
                locator.recoverable_inputs[index].clone()
            })
        })
    }
    pub fn child_action(&self, id: &str, child: &str, owner: &str, open: bool) -> Result<()> {
        if owner.is_empty() || owner.len() > 128 { bail!("子会话订阅标识无效"); }
        self.locator(id)?;
        let live = self.live(id)?;
        let guard = live.lifecycle.lock().unwrap();
        let view = live.view.lock().unwrap();
        if !view.connected || (open && view.pending.is_some()) || live.stopped.load(Ordering::Acquire) { bail!("ZCode Desktop 未连接或等待审批，暂不能读取子会话"); }
        let deferred_close = !open && view.pending.is_some();
        drop(view);
        let (tx, rx) = mpsc::sync_channel(1);
        live.sender.lock().unwrap().as_ref().context("ZCode Desktop 连接已关闭")?.send(Action::Child(child.into(), owner.into(), open, tx)).context("子会话请求发送失败")?;
        drop(guard);
        if deferred_close { return Ok(()); } // Retire after approval; never block closing the read-only UI.
        rx.recv_timeout(Duration::from_secs(65)).context("子会话请求超时")?.map_err(|error| anyhow!(error))
    }
    pub fn composer_catalog(&self, id: &str) -> Result<Value> {
        let cwd = self.locator(id)?.cwd;
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        let view = live.view.lock().unwrap();
        if !view.connected || view.pending.is_some() || live.stopped.load(Ordering::Acquire) {
            bail!("ZCode Desktop 未连接或正在等待审批，暂不能刷新能力目录");
        }
        drop(view);
        let (tx, rx) = mpsc::sync_channel(1);
        live.sender
            .lock()
            .unwrap()
            .as_ref()
            .context("ZCode Desktop 连接已关闭")?
            .send(Action::Catalog(cwd, tx))
            .context("ZCode Desktop 目录请求发送失败")?;
        drop(_life);
        rx.recv_timeout(Duration::from_secs(195))
            .context("ZCode Desktop 目录加载等待超时")
    }
    pub fn attachment_info(&self, id: &str, paths: &[String]) -> Result<Vec<Value>> {
        self.locator(id)?;
        prepare_attachments(paths)
    }
    pub fn set_model(&self, id: &str, model_id: &str) -> Result<ZCodeSnapshot> {
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        {
            let view = live.view.lock().unwrap();
            if !view.connected || view.busy || live.stopped.load(Ordering::Acquire) {
                bail!("ZCode Desktop 未连接或正在执行，暂不能切换模型")
            }
            if !view.models.iter().any(|model| model.id == model_id) {
                bail!("所选模型不在 ZCode Desktop Personal API 目录中")
            }
        }
        let (tx, rx) = mpsc::sync_channel(1);
        live.sender
            .lock()
            .unwrap()
            .as_ref()
            .context("ZCode Desktop 连接已关闭")?
            .send(Action::SetModel(model_id.into(), tx))
            .context("ZCode Desktop 模型选择发送失败")?;
        rx.recv_timeout(Duration::from_secs(65))
            .context("ZCode Desktop 模型选择等待超时")?
            .map_err(anyhow::Error::msg)?;
        self.snapshot(id)
    }
    pub fn set_permission_mode(&self, id: &str, mode: &str) -> Result<ZCodeSnapshot> {
        if !zcode::valid_permission_mode(mode) {
            bail!("ZCode Desktop 权限模式无效");
        }
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        let view = live.view.lock().unwrap();
        if !view.connected
            || view.busy
            || view.pending.is_some()
            || live.stopped.load(Ordering::Acquire)
        {
            bail!("ZCode Desktop 未连接或正在执行，暂不能切换权限模式");
        }
        drop(view);
        let (tx, rx) = mpsc::sync_channel(1);
        live.sender
            .lock()
            .unwrap()
            .as_ref()
            .context("ZCode Desktop 连接已关闭")?
            .send(Action::SetMode(mode.into(), tx))
            .context("ZCode Desktop 权限选择发送失败")?;
        rx.recv_timeout(Duration::from_secs(65))
            .context("ZCode Desktop 权限选择等待超时")?
            .map_err(anyhow::Error::msg)?;
        self.snapshot(id)
    }
    pub fn edit_user_query(&self, id: &str, row_id: u64, entity_id: &str, text: &str) -> Result<String> {
        self.admit_edit(id, row_id, entity_id, text)?.wait()
    }
    pub(crate) fn admit_edit(&self, id: &str, row_id: u64, entity_id: &str, text: &str) -> Result<PendingZCodeInput> {
        if entity_id.trim().is_empty() || text.chars().count() > 32000 { bail!("编辑消息标识无效或超过 32000 字符"); }
        let rows = self.snapshot(id)?.rows;
        let live = self.live(id)?;
        let life = live.lifecycle.lock().unwrap();
        let view = live.view.lock().unwrap();
        if !view.connected || live.stopped.load(Ordering::Acquire) || view.pending.is_some() || view.selected_model_id.is_none() { bail!("请连接会话、选择模型并先处理工具审批"); }
        let row = rows.iter().find(|row| row["rowId"] == row_id && row["kind"] == "userInput" && row["entityId"] == entity_id).context("待编辑消息已变更，请刷新后重试")?;
        if text.trim().is_empty() && !row["attachments"].as_array().is_some_and(|items| !items.is_empty()) { bail!("编辑消息不能为空"); }
        drop(view);
        let (tx, rx) = mpsc::sync_channel(1);
        live.sender.lock().unwrap().as_ref().context("ZCode Desktop 连接已关闭")?.send(Action::Edit(row_id, entity_id.into(), text.into(), tx)).context("编辑请求发送失败")?;
        drop(life);
        Ok(PendingZCodeInput { response: rx, uncertain_message: "编辑结果等待超时，请核对历史后重试" })
    }
    pub fn cancel(&self, id: &str) -> Result<bool> {
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        if !live.view.lock().unwrap().busy {
            return Ok(false);
        }
        live.cancel.store(true, Ordering::Release);
        self.reject(&live);
        live.sender
            .lock()
            .unwrap()
            .as_ref()
            .context("ZCode Desktop 连接已关闭")?
            .send(Action::Cancel)
            .context("ZCode Desktop 取消发送失败")?;
        Ok(true)
    }
    pub fn answer(&self, id: &str, request: &str, allow: bool) -> Result<()> {
        let live = self.live(id)?;
        let mut view = live.view.lock().unwrap();
        let (params, sender) = view.pending.as_ref().context("审批已结束")?;
        if params["requestId"].as_str() != Some(request) {
            bail!("ZCode Desktop 审批标识不匹配");
        }
        sender.try_send(allow).context("ZCode Desktop 审批已回答或结束")?;
        view.pending = None;
        Ok(())
    }
    fn reject(&self, live: &Live) {
        if let Some((_, sender)) = live.view.lock().unwrap().pending.take() {
            let _ = sender.try_send(false);
        }
    }
    pub fn stop<R: tauri::Runtime>(
        &self,
        id: &str,
        sessions: &SessionManager,
        app: &AppHandle<R>,
    ) -> Result<SessionRecord> {
        self.stop_inner(id, sessions, app)
    }
    pub(crate) fn stop_for_handoff<R: tauri::Runtime>(&self, id: &str, sessions: &SessionManager, app: &AppHandle<R>) -> Result<SessionRecord> { self.stop_inner(id, sessions, app) }
    fn stop_inner<R: tauri::Runtime>(&self, id: &str, sessions: &SessionManager, app: &AppHandle<R>) -> Result<SessionRecord> {
        let live = self.live(id)?;
        let _life = live.lifecycle.lock().unwrap();
        // Preserve the active connection on unverified stop. Setting stopped
        // first lets the worker exit and drop its identity before failure returns.
        crate::acp::terminate_acp_process_checked(&live.process_id)?;
        live.stopped.store(true, Ordering::Release);
        self.reject(&live);
        live.sender.lock().unwrap().take();
        if let Some(worker) = live.worker.lock().unwrap().take() {
            worker.join().map_err(|_| anyhow!("ZCode Desktop 工作线程异常"))?;
        }
        crate::acp::terminate_acp_process_checked(&live.process_id)?;
        {
            let mut view = live.view.lock().unwrap();
            view.connected = false;
            view.busy = false;
        }
        let record = sessions.mark_native_runtime_status(id, SessionStatus::Stopped, None, None)?;
        self.publish(app, id, &live, Some(record.clone()))?;
        Ok(record)
    }
    pub fn stop_all<R: tauri::Runtime>(
        &self,
        sessions: &SessionManager,
        app: &AppHandle<R>,
    ) -> Result<()> {
        for record in sessions.list().into_iter().filter(is_standalone_zcode) {
            let lifecycle = sessions.lifecycle_lock(&record.id);
            let _guard = lifecycle.lock().unwrap();
            if self.locator(&record.id).is_ok() {
                self.stop(&record.id, sessions, app)?;
            }
        }
        Ok(())
    }
    pub fn delete_with<T>(&self, id: &str, delete: impl FnOnce() -> Result<T>) -> Result<T> {
        let locator = self.locator(id)?;
        self.mutate(|store| {
            store.sessions.remove(id);
            Ok(())
        })?;
        match delete() {
            Ok(value) => {
                self.live.lock().unwrap().remove(id);
                Ok(value)
            }
            Err(error) => {
                self.mutate(|store| {
                    store.sessions.insert(id.into(), locator);
                    Ok(())
                })?;
                Err(error)
            }
        }
    }
}
/// Title failures must not fail message delivery. Retry a failed title only after
/// new metadata or reconnect, rather than writing/logging every 50 ms poll.
fn sync_session_title(
    sessions: &SessionManager,
    id: &str,
    title: Option<&str>,
    observed: &mut Option<String>,
) -> Option<SessionRecord> {
    let title = title?;
    if observed.as_deref() == Some(title) {
        return None;
    }
    *observed = Some(title.into());
    match sessions.apply_summary_title(id, title) {
        Ok(record) => record,
        Err(_) => {
            eprintln!("[crewvela] ZCode 会话标题保存失败 session={id}，保留原名称");
            None
        }
    }
}

fn save(path: &Path, store: &Store) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(store)?;
    if bytes.len() > 64 * 1024 * 1024 {
        bail!("ZCode Desktop 会话存储超过 64MB 安全上限");
    }
    let temp = path.with_extension(format!("zcode-{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_zcode_titles_sync_persist_restore_and_respect_manual_names() {
        let (root, manager, sessions, record, app) = fixture("titles");
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert_eq!(sessions.get(&record.id).unwrap().title, record.title);
        manager.send(&record.id, "请修复登录接口的超时问题").unwrap();
        wait(&manager, &record.id, |_| sessions.get(&record.id).unwrap().title == "会话摘要1");
        let reopened = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        assert_eq!(reopened.get(&record.id).unwrap().title, "会话摘要1");
        sessions.rename(&record.id, "用户手动名称").unwrap();
        manager.send(&record.id, "再检查重试逻辑").unwrap();
        wait(&manager, &record.id, |s| !s.busy && s.rows.iter().any(|r| r["rowId"] == 24));
        assert_eq!(sessions.get(&record.id).unwrap().title, "用户手动名称");
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        manager.start(app.handle().clone(), &sessions, &sessions.get(&record.id).unwrap()).unwrap();
        assert_eq!(sessions.get(&record.id).unwrap().title, "用户手动名称");
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();

        let (root, manager, sessions, record, app) = fixture("titles");
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let mut reopened = NativeZCodeSessions::open_at(root.join("zcode.json")).unwrap();
        reopened.test_peer = manager.test_peer.clone();
        reopened.start(app.handle().clone(), &sessions, &sessions.get(&record.id).unwrap()).unwrap();
        assert_eq!(sessions.get(&record.id).unwrap().title, "原生已保存摘要");
        reopened.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_zcode_titles_failed_save_keeps_name_and_skips_duplicate_writes() {
        let (root, _manager, sessions, record, _app) = fixture("normal");
        let mut observed = None;
        fs::remove_file(root.join("sessions.json")).unwrap();
        fs::create_dir(root.join("sessions.json")).unwrap();
        assert!(sync_session_title(&sessions, &record.id, Some("新摘要"), &mut observed).is_none());
        assert_eq!(sessions.get(&record.id).unwrap().title, record.title);
        assert_eq!(sessions.get(&record.id).unwrap().updated_at, record.updated_at);
        fs::remove_dir(root.join("sessions.json")).unwrap();
        assert!(sync_session_title(&sessions, &record.id, Some("新摘要"), &mut observed).is_none());
        assert!(!root.join("sessions.json").exists());
        // Reconnect retries the same authoritative metadata.
        observed = None;
        assert!(sync_session_title(&sessions, &record.id, Some("新摘要"), &mut observed).is_some());
        assert!(sessions.apply_summary_title(&record.id, "新摘要").unwrap().is_none());
        let reopened = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        assert_eq!(reopened.get(&record.id).unwrap().title, "新摘要");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_child_close_during_approval_defers_without_blocking() {
        let (root, manager, sessions, record, app) = fixture("subagents_permission");
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        manager.child_action(&record.id, "child-a", "owner", true).unwrap();
        manager.send(&record.id, "permission").unwrap();
        let waiting = wait(&manager, &record.id, |s| s.pending_permission.is_some());
        let began = Instant::now();
        manager.child_action(&record.id, "child-a", "owner", false).unwrap();
        assert!(began.elapsed() < Duration::from_secs(1));
        assert!(manager.snapshot(&record.id).unwrap().pending_permission.is_some());
        manager.answer(&record.id, waiting.pending_permission.unwrap()["requestId"].as_str().unwrap(), false).unwrap();
        wait(&manager, &record.id, |s| s.child_detail.is_none());
        assert!(manager.snapshot(&record.id).unwrap().connected);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
        let (root, manager, sessions, record, app) = fixture("subagents_bad_ack");
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(manager.child_action(&record.id, "child-a", "owner", true).is_err());
        thread::sleep(Duration::from_millis(150));
        let failed = manager.snapshot(&record.id).unwrap();
        assert!(failed.connected && failed.child_detail.is_none());
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_child_identity_projection_and_close_are_isolated() {
        let (root, manager, sessions, record, app) = fixture("subagents");
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert!(manager.child_action(&record.id, "unrelated", "test-a", true).is_err());
        manager.child_action(&record.id, "child-a", "test-a", true).unwrap();
        let detail = wait(&manager, &record.id, |snapshot| snapshot.child_detail.as_ref().is_some_and(|detail| detail["loading"] == false));
        assert_eq!(detail.child_detail.unwrap()["rows"][0]["text"], "child answer");
        assert!(detail.rows.iter().all(|row| row["text"] != "child answer"));
        manager.child_action(&record.id, "child-b", "test-b", true).unwrap();
        let next = wait(&manager, &record.id, |snapshot| snapshot.child_detail.as_ref().is_some_and(|detail| detail["childSessionId"] == "child-b" && detail["loading"] == false));
        assert!(next.child_detail.unwrap()["error"].is_null());
        manager.child_action(&record.id, "child-a", "test-a", false).unwrap();
        assert_eq!(manager.snapshot(&record.id).unwrap().child_detail.unwrap()["childSessionId"], "child-b");
        manager.child_action(&record.id, "child-b", "test-b", false).unwrap();
        manager.child_action(&record.id, "child-a", "test-old", true).unwrap();
        manager.child_action(&record.id, "child-a", "test-new", true).unwrap();
        manager.child_action(&record.id, "child-a", "test-old", false).unwrap();
        assert!(manager.snapshot(&record.id).unwrap().child_detail.is_some());
        manager.child_action(&record.id, "child-a", "test-new", false).unwrap();
        let closed = manager.snapshot(&record.id).unwrap();
        assert!(closed.connected && closed.child_detail.is_none());
        thread::sleep(Duration::from_millis(150));
        assert!(manager.snapshot(&record.id).unwrap().connected);
        assert_eq!(sessions.get(&record.id).unwrap().title, record.title);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_failed_status_save_preserves_previous_record() {
        let (root, _manager, sessions, record, _app) = fixture("normal");
        fs::remove_file(root.join("sessions.json")).unwrap();
        fs::create_dir(root.join("sessions.json")).unwrap();
        assert!(sessions
            .mark_native_runtime_status(&record.id, SessionStatus::Running, None, None)
            .is_err());
        assert_eq!(sessions.get(&record.id).unwrap().status, record.status);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_empty_resume_requires_confirmed_server_absence() {
        for scenario in ["missing", "resume_lineage_missing", "resume_lineage_existing", "resume_lineage_list_error", "resume_lineage_malformed", "resume_internal"] {
            let (root, manager, sessions, record, app) = fixture(scenario);
            manager.start(app.handle().clone(), &sessions, &record).unwrap();
            manager.stop(&record.id, &sessions, app.handle()).unwrap();
            manager.mutate(|store| { store.sessions.get_mut(&record.id).unwrap().rows.clear(); Ok(()) }).unwrap();
            let before = manager.locator(&record.id).unwrap();
            let result = manager.start(app.handle().clone(), &sessions, &sessions.get(&record.id).unwrap());
            if matches!(scenario, "missing" | "resume_lineage_missing") {
                assert!(result.unwrap().connected, "{scenario}");
            } else {
                assert!(result.is_err(), "must preserve unverified session: {scenario}");
                let after = manager.locator(&record.id).unwrap();
                assert_eq!(after.native_session_id, before.native_session_id);
                assert_eq!(after.model_selection, before.model_selection);
                assert_eq!(after.rows, before.rows);
            }
            manager.stop(&record.id, &sessions, app.handle()).unwrap();
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn native_zcode_missing_history_fails_without_recreating() {
        for scenario in ["missing", "resume_lineage_history", "resume_internal"] {
        let (root, manager, sessions, record, app) = fixture(scenario);
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let before = manager.locator(&record.id).unwrap();
        assert!(!before.rows.is_empty());
        assert!(manager
            .start(
                app.handle().clone(),
                &sessions,
                &sessions.get(&record.id).unwrap()
            )
            .is_err());
        assert_eq!(manager.locator(&record.id).unwrap().rows, before.rows);
        assert_eq!(manager.locator(&record.id).unwrap().native_session_id, before.native_session_id);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
        }
    }
    fn fixture(
        scenario: &str,
    ) -> (
        PathBuf,
        NativeZCodeSessions,
        SessionManager,
        SessionRecord,
        tauri::App<tauri::test::MockRuntime>,
    ) {
        let root = std::env::temp_dir().join(format!("cc-native-zcode-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let record = sessions
            .create_runtime(crate::sessions::NewRuntimeSession {
                runtime_kind: "zcode".into(),
                cwd: root.display().to_string(),
                project_id: None,
            })
            .unwrap();
        let mut manager = NativeZCodeSessions::open_at(root.join("zcode.json")).unwrap();
        manager.test_peer = Some((
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/native-zcode-agent.mjs"),
            scenario.into(),
        ));
        manager.register(&record.id, &record.cwd).unwrap();
        (root, manager, sessions, record, tauri::test::mock_app())
    }
    fn wait(
        manager: &NativeZCodeSessions,
        id: &str,
        predicate: impl Fn(&ZCodeSnapshot) -> bool,
    ) -> ZCodeSnapshot {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let snapshot = manager.snapshot(id).unwrap();
            if predicate(&snapshot) {
                return snapshot;
            }
            assert!(Instant::now() < deadline, "ZCode Desktop state timed out");
            thread::sleep(Duration::from_millis(20));
        }
    }
    #[test]
    fn native_zcode_catalog_errors_are_visible_without_losing_other_kinds() {
        let (root, manager, sessions, record, app) = fixture("catalog_error");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        let catalog = manager.composer_catalog(&record.id).unwrap();
        assert!(catalog["skills"]["error"]
            .as_str()
            .unwrap()
            .contains("格式"));
        assert!(catalog["plugins"]["error"].is_string());
        assert_eq!(catalog["commands"]["items"][0]["name"], "init");
        assert!(catalog["commands"]["notice"].is_string());
        assert!(manager.snapshot(&record.id).unwrap().connected);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_edit_uses_canonical_target_and_cas_preserves_connection_on_rejection() {
        let (root, manager, sessions, record, app) = fixture("edit");
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        let attachment = root.join("edit attachment.txt");
        fs::write(&attachment, "keep this file").unwrap();
        manager.send_input(&record.id, "original", &[attachment.display().to_string()], false).unwrap();
        wait(&manager, &record.id, |s| !s.busy && s.rows.iter().any(|r| r["entityId"] == "user-1"));
        assert!(manager.edit_user_query(&record.id, 12, "wrong", "replacement").is_err());
        assert!(manager.edit_user_query(&record.id, 12, "user-1", "reject-edit").unwrap_err().to_string().contains("proto.staleRevision"));
        assert!(manager.snapshot(&record.id).unwrap().connected);
        let quoted = "replacement\n\n# userselect:\n```userselect\n[{\"text\":\"selected answer\"}]\n```";
        manager.edit_user_query(&record.id, 12, "user-1", quoted).unwrap();
        let edited = wait(&manager, &record.id, |s| !s.busy && s.rows.iter().any(|r| r["text"] == quoted));
        assert!(edited.connected);
        assert!(!edited.rows.iter().any(|r| r["rowId"] == 13 || r["rowId"] == 14), "old reasoning and answer must be removed by native rewind events");
        assert!(edited.rows.iter().any(|r| r["rowId"] == 16 && r["text"] == "edited answer"));
        assert!(edited.rows.iter().any(|r| r["rowId"] == 1), "earlier history remains");
        manager.edit_user_query(&record.id, 12, "user-1", "").unwrap();
        wait(&manager, &record.id, |s| s.busy && s.rows.iter().any(|r| r["rowId"] == 12 && r["text"] == ""));
        assert!(manager.cancel(&record.id).unwrap());
        let cancelled = wait(&manager, &record.id, |s| !s.busy);
        assert!(cancelled.connected);
        assert!(cancelled.rows.iter().any(|r| r["rowId"] == 12 && r["attachments"][0]["fileName"] == "edit attachment.txt"));
        assert!(cancelled.rows.iter().any(|r| r["kind"] == "turnHeader" && r["state"] == "completedInterrupted"));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_catalog_and_attachment_only_round_trip() {
        let (root, manager, sessions, record, app) = fixture("attachments");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        let catalog = manager.composer_catalog(&record.id).unwrap();
        assert_eq!(catalog["skills"]["items"][0]["name"], "fixture");
        assert_eq!(catalog["commands"]["items"][0]["name"], "init");
        assert_eq!(catalog["commands"]["items"].as_array().unwrap().len(), 1);
        assert_eq!(catalog["plugins"]["items"], json!([]));
        assert!(catalog["skills"].get("error").is_none());
        let path = root.join("native attachment.txt");
        fs::write(&path, b"fixture").unwrap();
        assert!(manager.send_input(&record.id, "", &[], false).is_err());
        manager
            .send_input(&record.id, "", &[path.display().to_string()], false)
            .unwrap();
        let snapshot = wait(&manager, &record.id, |view| !view.busy);
        let row = snapshot
            .rows
            .iter()
            .find(|row| row["kind"] == "userInput")
            .unwrap();
        assert_eq!(row["text"], "");
        assert_eq!(row["attachments"][0]["fileName"], "native attachment.txt");
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let stored = NativeZCodeSessions::open_at(root.join("zcode.json"))
            .unwrap()
            .snapshot(&record.id)
            .unwrap();
        assert!(stored
            .rows
            .iter()
            .any(|row| row["attachments"][0]["bytes"] == 7));
        assert!(manager.composer_catalog(&record.id).is_err());
        assert!(prepare_attachments(&["relative.txt".into()]).is_err());
        assert!(prepare_attachments(&[root.display().to_string()]).is_err());
        assert!(prepare_attachments(&vec![path.display().to_string(); 9]).is_err());
        let large = root.join("large.bin");
        fs::File::create(&large)
            .unwrap()
            .set_len(20 * 1024 * 1024 + 1)
            .unwrap();
        assert!(prepare_attachments(&[large.display().to_string()]).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_multiple_turns_and_cross_process_restore() {
        let (root, manager, sessions, record, app) = fixture("normal");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        for message in ["first", "second"] {
            manager.send(&record.id, message).unwrap();
            let snapshot = wait(&manager, &record.id, |s| !s.busy);
            assert!(snapshot
                .rows
                .iter()
                .any(|r| r["kind"] == "userInput" && r["text"] == message));
            assert!(snapshot
                .rows
                .iter()
                .any(|r| r["kind"] == "reasoning" && r["text"] == "thinking"));
        }
        assert_eq!(sessions.drain_transitions().into_iter().filter(|event| event.to == SessionStatus::Completed).count(), 2);
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let mut reopened = NativeZCodeSessions::open_at(root.join("zcode.json")).unwrap();
        reopened.test_peer = manager.test_peer.clone();
        assert_eq!(
            reopened
                .locator(&record.id)
                .unwrap()
                .native_session_id
                .as_deref(),
            Some("native-fixture")
        );
        reopened
            .start(
                app.handle().clone(),
                &sessions,
                &sessions.get(&record.id).unwrap(),
            )
            .unwrap();
        assert!(sessions.drain_transitions().iter().all(|event| event.to != SessionStatus::Completed), "restored history must not replay completion automations");
        reopened.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(windows)]
    #[test]
    fn native_zcode_unverified_stop_preserves_live_identity_and_can_retry() {
        let (root, manager, sessions, record, app) = fixture("normal");
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        let live = manager.live(&record.id).unwrap();
        let original_pid = live.process_id.load(Ordering::Acquire);
        let previous_status = sessions.get(&record.id).unwrap().status;
        // An unregistered identity is rejected before touching any process. Never
        // reopen or terminate this synthetic PID; restore the owned identity.
        live.process_id.store(u32::MAX, Ordering::Release);
        let error = manager.stop(&record.id, &sessions, app.handle()).err().expect("unknown identity must refuse stop");
        assert!(error.downcast_ref::<crate::acp::UnverifiedStop>().is_some());
        assert!(!live.stopped.load(Ordering::Acquire));
        assert!(manager.snapshot(&record.id).unwrap().connected);
        assert!(live.sender.lock().unwrap().is_some());
        assert_eq!(sessions.get(&record.id).unwrap().status, previous_status);
        live.process_id.store(original_pid, Ordering::Release);
        manager.send(&record.id, "after unverified stop").unwrap();
        assert!(wait(&manager, &record.id, |snapshot| !snapshot.busy).connected);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Stopped);
        assert_eq!(live.process_id.load(Ordering::Acquire), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn native_zcode_disconnected_unverified_process_blocks_replacement_until_cleanup() {
        let (root, manager, sessions, record, app) = fixture("model_bad_frame");
        let first = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        let live = manager.live(&record.id).unwrap();
        let pid = live.process_id.load(Ordering::Acquire);
        let refusal = crate::acp::TestStopFailureGuard::new(pid);
        assert!(manager.set_model(&record.id, first.selected_model_id.as_ref().unwrap()).is_err());
        wait(&manager, &record.id, |snapshot| !snapshot.connected);
        let error = manager.start(app.handle().clone(), &sessions, &record).err().expect("unverified old process must block recovery");
        assert!(error.downcast_ref::<crate::acp::UnverifiedStop>().is_some(), "{error:#}");
        assert_eq!(live.process_id.load(Ordering::Acquire), pid);
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        assert!(live.worker.lock().unwrap().is_none(), "no replacement worker may be admitted");
        drop(refusal);
        manager.start(app.handle().clone(), &sessions, &record).unwrap();
        assert_ne!(live.process_id.load(Ordering::Acquire), pid);
        manager.send(&record.id, "recovered after verified cleanup").unwrap();
        wait(&manager, &record.id, |snapshot| !snapshot.busy);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_zcode_closed_runtime_admission_rejects_start_without_process() {
        let (root, manager, sessions, record, app) = fixture("normal");
        let admission = crate::runtime_admission::RuntimeAdmission::default();
        app.manage(admission.clone());
        assert!(manager.start(app.handle().clone(), &sessions, &record).unwrap().connected);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        admission.begin_shutdown(Duration::from_secs(1)).unwrap().commit();
        let error = manager.start(app.handle().clone(), &sessions, &record).err().expect("closed gate must refuse startup");
        assert!(error.to_string().contains("正在退出"));
        let live = manager.live(&record.id).unwrap();
        assert_eq!(live.process_id.load(Ordering::Acquire), 0);
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_zcode_server_model_rejection_keeps_previous_model_and_send_usable() {
        let (root, manager, sessions, record, app) = fixture("model_rejected");
        let first = manager.start(app.handle().clone(), &sessions, &record).unwrap();
        let previous = first.selected_model_id.clone().unwrap();
        let persisted = manager.locator(&record.id).unwrap().model_selection;
        let rejected = first.models.iter().find(|model| model.id != previous).unwrap();
        // This ID is advertised and passes the local gate; the fixture must
        // receive session/setModel and reject it via an actual JSON-RPC error.
        let error = manager.set_model(&record.id, &rejected.id).err().expect("the server must reject this model").to_string();
        assert!(error.contains("session/setModel") && error.contains("-32000"), "must be the native RPC rejection: {error}");
        let after = manager.snapshot(&record.id).unwrap();
        assert!(after.connected);
        assert_eq!(after.selected_model_id, Some(previous));
        assert_eq!(manager.locator(&record.id).unwrap().model_selection, persisted);
        manager.send(&record.id, "send after rejected model").unwrap();
        let settled = wait(&manager, &record.id, |snapshot| !snapshot.busy);
        assert!(settled.connected);
        assert!(settled.rows.iter().any(|row| row["kind"] == "userInput" && row["text"] == "send after rejected model"));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn native_zcode_model_transport_and_persistence_failures_still_disconnect() {
        for transport in [false, true] {
            let (root, manager, sessions, record, app) = fixture(if transport { "model_disconnect" } else { "normal" });
            let first = manager.start(app.handle().clone(), &sessions, &record).unwrap();
            if !transport {
                fs::remove_file(root.join("zcode.json")).unwrap();
                fs::create_dir(root.join("zcode.json")).unwrap();
            }
            assert!(manager.set_model(&record.id, &first.models[0].id).is_err());
            let after = wait(&manager, &record.id, |snapshot| !snapshot.connected);
            assert!(after.error.is_some());
            assert!(manager.send(&record.id, "must not send").is_err());
            manager.stop(&record.id, &sessions, app.handle()).unwrap();
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn native_zcode_model_gate_selection_and_rejection_keep_connection() {
        let (root, manager, sessions, record, app) = fixture("unselected");
        let first = manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        assert_eq!(first.models.len(), 1);
        assert!(first.selected_model_id.is_none());
        assert!(manager.send(&record.id, "blocked").is_err());
        assert!(manager.set_model(&record.id, "foreign").is_err());
        let chosen = manager.set_model(&record.id, &first.models[0].id).unwrap();
        assert_eq!(
            chosen.selected_model_id.as_deref(),
            Some(first.models[0].id.as_str())
        );
        manager.send(&record.id, "allowed").unwrap();
        wait(&manager, &record.id, |s| !s.busy);
        assert!(manager
            .locator(&record.id)
            .unwrap()
            .model_selection
            .is_some());
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let mut reopened = NativeZCodeSessions::open_at(root.join("zcode.json")).unwrap();
        reopened.test_peer = manager.test_peer.clone();
        let restored = reopened
            .start(
                app.handle().clone(),
                &sessions,
                &sessions.get(&record.id).unwrap(),
            )
            .unwrap();
        assert_eq!(restored.selected_model_id, chosen.selected_model_id);
        reopened.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
        let (root, manager, sessions, record, app) = fixture("rejected");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        assert!(manager.send(&record.id, "rejected").is_err());
        let rejected = wait(&manager, &record.id, |s| s.error.is_some());
        assert!(rejected.connected && !rejected.busy);
        assert!(!rejected.error.unwrap().contains("fixture-secret"));
        manager.send(&record.id, "retry").unwrap();
        wait(&manager, &record.id, |s| !s.busy);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_queue_cas_conflict_does_not_mutate_or_resume() {
        let (root, manager, sessions, record, app) = fixture("queue_cas_conflict");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        manager.send(&record.id, "current").unwrap();
        let queued = manager.send_input(&record.id, "next", &[], false).unwrap();
        let error = manager
            .queue_action(&record.id, Some(&queued))
            .err().expect("CAS conflict must be rejected")
            .to_string();
        assert!(error.contains("proto.staleRevision"));
        let view = manager.snapshot(&record.id).unwrap();
        assert!(view.connected && view.busy);
        assert_eq!(view.queued_inputs.len(), 1);
        manager.cancel(&record.id).unwrap();
        wait(&manager, &record.id, |view| {
            !view.busy && !view.queue_auto_drain
        });
        assert!(manager
            .queue_action(&record.id, None)
            .err().expect("CAS conflict must be rejected")
            .to_string()
            .contains("proto.staleRevision"));
        let view = manager.snapshot(&record.id).unwrap();
        assert!(view.connected && !view.busy && !view.queue_auto_drain);
        assert_eq!(view.queued_inputs.len(), 1);
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_queue_multi_send_reject_cancel_resume_and_busy_catalog() {
        let (root, manager, sessions, record, app) = fixture("queue");
        let initial = manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        assert!(initial.can_queue && !initial.busy);
        assert!(manager
            .send_input(
                &record.id,
                "attachment vanished",
                &[root.join("missing.txt").display().to_string()],
                false
            )
            .unwrap_err()
            .to_string()
            .starts_with("ZCODE_INPUT_REJECTED:"));
        assert!(manager
            .snapshot(&record.id)
            .unwrap()
            .recoverable_inputs
            .is_empty());
        assert!(manager
            .send_input(&record.id, "rpc-reject", &[], false)
            .unwrap_err()
            .to_string()
            .starts_with("ZCODE_INPUT_REJECTED:"));
        assert!(!manager.snapshot(&record.id).unwrap().busy);
        manager.send(&record.id, "current").unwrap();
        assert!(manager.snapshot(&record.id).unwrap().busy);
        assert!(manager.composer_catalog(&record.id).unwrap()["skills"]["items"].is_array());
        let attachment = root.join("queued.txt");
        fs::write(&attachment, "fixture").unwrap();
        assert_eq!(
            manager
                .attachment_info(&record.id, &[attachment.display().to_string()])
                .unwrap()
                .len(),
            1
        );
        let second = manager
            .send_input(&record.id, "second", &[attachment.display().to_string()], false)
            .unwrap();
        let third = manager.send_input(&record.id, "third", &[], false).unwrap();
        let queued = manager.snapshot(&record.id).unwrap();
        assert_eq!(queued.queued_inputs.len(), 2);
        assert!(queued.busy);
        assert_eq!(queued.queued_inputs[0]["sourceCommandId"], second);
        assert_eq!(
            queued.queued_inputs[0]["attachments"][0]["fileName"],
            "queued.txt"
        );
        assert!(manager.send_input(&record.id, "reject", &[], false).is_err());
        let rejection = manager
            .send_input(&record.id, "rpc-reject", &[], false)
            .unwrap_err()
            .to_string();
        assert!(rejection.starts_with("ZCODE_INPUT_REJECTED:"));
        assert!(!rejection.contains("fixture-secret"));
        assert!(manager.snapshot(&record.id).unwrap().connected);
        assert!(manager.snapshot(&record.id).unwrap().busy);
        assert_eq!(manager.snapshot(&record.id).unwrap().queued_inputs.len(), 2);
        let withdrawn = manager.queue_action(&record.id, Some(&second)).unwrap();
        assert_eq!(withdrawn.queued_inputs.len(), 1);
        assert!(withdrawn.busy);
        manager.cancel(&record.id).unwrap();
        let paused = wait(&manager, &record.id, |view| {
            !view.busy && !view.queue_auto_drain
        });
        assert_eq!(paused.queued_inputs.len(), 1);
        assert!(sessions.drain_transitions().iter().all(|event| !matches!(event.to, SessionStatus::Completed | SessionStatus::Error)), "cancelled foreground turn must not fire terminal automations");
        assert_eq!(paused.input_routing.as_deref(), Some("choice"));
        assert!(manager.send(&record.id, "held must reject").is_err());
        manager.queue_action(&record.id, None).unwrap();
        let finished = wait(&manager, &record.id, |view| {
            !view.busy
                && view.queued_inputs.is_empty()
                && view.rows.iter().any(|row| {
                    row["kind"] == "turnHeader"
                        && row["sourceCommandId"] == third
                        && row["state"] == "completedSuccess"
                })
        });
        assert!(!finished.busy && finished.queued_inputs.is_empty());
        assert_eq!(sessions.drain_transitions().into_iter().filter(|event| event.to == SessionStatus::Completed).count(), 1, "auto-drained queued turn must produce its own completion event");
        assert!(finished.rows.iter().any(|row| row["kind"] == "turnHeader"
            && row["sourceCommandId"] == third
            && row["state"] == "completedSuccess"));
        assert!(!finished
            .rows
            .iter()
            .any(|row| row["kind"] == "userInput" && row["sourceCommandId"] == second));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_queue_delayed_confirmation_stop_recovery_never_auto_resends() {
        let (root, manager, sessions, record, app) = fixture("queue_delayed");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        manager.send(&record.id, "current").unwrap();
        let command = manager
            .send_input(&record.id, "recover draft", &[], false)
            .unwrap();
        assert_eq!(
            manager.locator(&record.id).unwrap().queued_inputs[0]["sourceCommandId"],
            command
        );
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let mut reopened = NativeZCodeSessions::open_at(root.join("zcode.json")).unwrap();
        reopened.test_peer = manager.test_peer.clone();
        let restored = reopened
            .start(
                app.handle().clone(),
                &sessions,
                &sessions.get(&record.id).unwrap(),
            )
            .unwrap();
        assert!(restored.queued_inputs.is_empty());
        assert_eq!(restored.recoverable_inputs.len(), 1);
        assert_eq!(restored.recoverable_inputs[0]["text"], "recover draft");
        assert!(!restored
            .rows
            .iter()
            .any(|row| row["sourceCommandId"] == command));
        assert_eq!(
            reopened
                .recover_queue_input(&record.id, &command, false)
                .unwrap()["text"],
            "recover draft"
        );
        assert_eq!(
            reopened
                .snapshot(&record.id)
                .unwrap()
                .recoverable_inputs
                .len(),
            1
        );
        reopened
            .recover_queue_input(&record.id, &command, true)
            .unwrap();
        assert!(reopened
            .snapshot(&record.id)
            .unwrap()
            .recoverable_inputs
            .is_empty());
        reopened.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_stop_and_cancel_available_while_queue_confirmation_waits() {
        let (root, manager, sessions, record, app) = fixture("queue_slow");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        manager.send(&record.id, "current").unwrap();
        std::thread::scope(|scope| {
            let pending = scope.spawn(|| manager.send_input(&record.id, "delayed", &[], false));
            // A fixed sleep can stop the connection before the sender has
            // acquired lifecycle under full-suite load (a definite rejection,
            // not an uncertain delivery). Observe the persisted submission first.
            wait(&manager, &record.id, |snapshot| snapshot.recoverable_inputs.iter().any(|input| input["text"] == "delayed"));
            assert!(!pending.is_finished(), "the fixture must still await native confirmation");
            let began = std::time::Instant::now();
            manager.stop(&record.id, &sessions, app.handle()).unwrap();
            assert!(began.elapsed() < Duration::from_secs(4));
            let error = pending.join().unwrap().unwrap_err().to_string();
            assert!(error.starts_with("ZCODE_INPUT_UNCERTAIN:"), "{error}");
        });
        assert!(!manager.snapshot(&record.id).unwrap().connected);
        let reopened = NativeZCodeSessions::open_at(root.join("zcode.json")).unwrap();
        let recovered = reopened.snapshot(&record.id).unwrap().recoverable_inputs;
        assert_eq!(recovered.len(), 1);
        assert_eq!(recovered[0]["text"], "delayed");
        assert!(recovered[0]["sourceCommandId"].is_string());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_cancel_reaches_peer_during_delayed_queue_confirmation() {
        let (root, manager, sessions, record, app) = fixture("queue_slow");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        let current = manager.send_input(&record.id, "current", &[], false).unwrap();
        std::thread::scope(|scope| {
            let pending = scope.spawn(|| manager.send_input(&record.id, "delayed", &[], false));
            std::thread::sleep(Duration::from_millis(100));
            let began = std::time::Instant::now();
            assert!(manager.cancel(&record.id).unwrap());
            pending.join().unwrap().unwrap();
            wait(&manager, &record.id, |snapshot| !snapshot.busy);
            assert!(
                began.elapsed() < Duration::from_secs(1),
                "native stop must arrive before the delayed queue frame"
            );
            let snapshot = manager.snapshot(&record.id).unwrap();
            assert!(!snapshot.queue_auto_drain);
            assert!(snapshot.rows.iter().any(|row| row["kind"] == "turnHeader"
                && row["sourceCommandId"] == current
                && row["state"] == "completedInterrupted"));
            assert_eq!(snapshot.queued_inputs.len(), 1);
        });
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_permission_modes_send_restore_and_rejection() {
        let (root, manager, sessions, record, app) = fixture("normal");
        let initial = manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        assert_eq!(initial.permission_mode.as_deref(), Some("build"));
        assert!(manager.set_permission_mode(&record.id, "auto").is_err());
        for mode in ["plan", "build", "edit", "yolo"] {
            let selected = manager.set_permission_mode(&record.id, mode).unwrap();
            assert_eq!(selected.permission_mode.as_deref(), Some(mode));
            manager.send(&record.id, "mode fixture").unwrap();
            wait(&manager, &record.id, |snapshot| !snapshot.busy);
        }
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let mut reopened = NativeZCodeSessions::open_at(root.join("zcode.json")).unwrap();
        reopened.test_peer = manager.test_peer.clone();
        let restored = reopened
            .start(
                app.handle().clone(),
                &sessions,
                &sessions.get(&record.id).unwrap(),
            )
            .unwrap();
        assert_eq!(restored.permission_mode.as_deref(), Some("yolo"));
        reopened.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
        let (root, manager, sessions, record, app) = fixture("mode_rejected");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        assert!(manager.set_permission_mode(&record.id, "yolo").is_err());
        let unchanged = manager.snapshot(&record.id).unwrap();
        assert!(unchanged.connected);
        assert_eq!(unchanged.permission_mode.as_deref(), Some("build"));
        assert_eq!(
            manager
                .locator(&record.id)
                .unwrap()
                .permission_mode
                .as_deref(),
            Some("build")
        );
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_unconfirmed_mode_does_not_persist() {
        let (root, manager, sessions, record, app) = fixture("mode_unconfirmed");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        assert!(manager.set_permission_mode(&record.id, "yolo").is_err());
        assert_eq!(
            manager
                .locator(&record.id)
                .unwrap()
                .permission_mode
                .as_deref(),
            Some("build")
        );
        assert_eq!(
            manager
                .snapshot(&record.id)
                .unwrap()
                .permission_mode
                .as_deref(),
            Some("build")
        );
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_permission_once_cancel_and_stop_cleanup() {
        let (root, manager, sessions, record, app) = fixture("permission");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        manager.send(&record.id, "approve").unwrap();
        let snapshot = wait(&manager, &record.id, |s| s.pending_permission.is_some());
        let request = snapshot.pending_permission.unwrap()["requestId"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(manager.send(&record.id, "concurrent").is_err());
        assert!(manager.set_permission_mode(&record.id, "yolo").is_err());
        assert!(manager.composer_catalog(&record.id).is_err());
        assert!(manager.answer(&record.id, "wrong", true).is_err());
        manager.answer(&record.id, &request, true).unwrap();
        assert!(manager.answer(&record.id, &request, true).is_err());
        wait(&manager, &record.id, |s| !s.busy);
        manager.send(&record.id, "cancel").unwrap();
        wait(&manager, &record.id, |s| s.pending_permission.is_some());
        manager.cancel(&record.id).unwrap();
        wait(&manager, &record.id, |s| !s.busy);
        assert!(manager.snapshot(&record.id).unwrap().connected);
        manager.send(&record.id, "stop").unwrap();
        wait(&manager, &record.id, |s| s.pending_permission.is_some());
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        let s = manager.snapshot(&record.id).unwrap();
        assert!(!s.connected && s.pending_permission.is_none());
        assert!(manager.answer(&record.id, "request-3", true).is_err());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_account_failure_retains_rows_and_connection() {
        let (root, manager, sessions, record, app) = fixture("account");
        manager
            .start(app.handle().clone(), &sessions, &record)
            .unwrap();
        manager.send(&record.id, "account").unwrap();
        let snapshot = wait(&manager, &record.id, |s| s.error.is_some());
        assert!(snapshot.connected);
        assert!(snapshot.error.unwrap().contains("账号模型"));
        assert!(snapshot.rows.iter().any(|r| r["text"] == "answer"));
        manager.stop(&record.id, &sessions, app.handle()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn native_zcode_failed_save_does_not_publish_store_changes() {
        let (root, mut manager, _sessions, record, _app) = fixture("normal");
        manager.path = root.clone();
        assert!(manager
            .mutate(|store| {
                store
                    .sessions
                    .get_mut(&record.id)
                    .unwrap()
                    .native_session_id = Some("changed".into());
                Ok(())
            })
            .is_err());
        assert!(manager
            .locator(&record.id)
            .unwrap()
            .native_session_id
            .is_none());
        fs::remove_dir_all(root).unwrap();
    }
}
