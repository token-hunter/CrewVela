use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::launcher::{resolve_claude, CLEAR_VARS};
use crate::providers::ProviderConfig;
use crate::pty::PtyManager;
use crate::sessions::{PermissionMode, SessionKind, SessionManager, SessionRecord};

pub const MAX_CONTROL_SESSIONS: u32 = 12;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(90);
const BACKTAB: &[u8] = b"\x1b[Z";
const MODE_STEP_DELAY: Duration = Duration::from_millis(120);

pub fn permission_mode_steps(
    current: PermissionMode,
    target: PermissionMode,
    bypass_enabled: bool,
) -> Option<usize> {
    if !target.is_unified_target() {
        return None;
    }

    let standard_cycle = [
        PermissionMode::Default,
        PermissionMode::AcceptEdits,
        PermissionMode::Plan,
        PermissionMode::Auto,
    ];
    let bypass_cycle = [
        PermissionMode::Default,
        PermissionMode::AcceptEdits,
        PermissionMode::Plan,
        PermissionMode::BypassPermissions,
        PermissionMode::Auto,
    ];
    let cycle = if bypass_enabled || current == PermissionMode::BypassPermissions {
        bypass_cycle.as_slice()
    } else {
        standard_cycle.as_slice()
    };
    let current_index = cycle.iter().position(|mode| *mode == current)?;
    let target_index = cycle.iter().position(|mode| *mode == target)?;
    Some((target_index + cycle.len() - current_index) % cycle.len())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlIntent {
    Launch,
    Broadcast,
    StopAll,
    DeleteAll,
    Help,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlLaunch {
    pub provider_id: String,
    pub count: u32,
    #[serde(default)]
    pub mode: ControlLaunchMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlLaunchMode {
    #[default]
    Default,
    Auto,
    Yolo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlPlan {
    pub intent: ControlIntent,
    pub reply: String,
    #[serde(default)]
    pub launches: Vec<ControlLaunch>,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ControlMessageRole {
    User,
    Assistant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlHistoryMessage {
    pub role: ControlMessageRole,
    pub content: String,
}

/// 会话间消息的发送方标记。None 表示人发起的广播/初始任务（投递时逐字不发包装）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageOrigin {
    pub session_id: String,
    pub label: String,
}

/// 消息类别：区分协作对话与启动初始任务、系统通知。
/// 协作工作台的「协作记录」只投影对话类投递，初始任务不算协作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MessageKind {
    /// agent 互发 / 总控 @ 定向 / 自动化 message 动作
    #[default]
    Chat,
    /// 广播（enqueue_broadcast，含自动化 broadcast 动作）
    Broadcast,
    /// 系统通知（限流停止通知）
    System,
    /// 会话启动初始任务（launch_one_session 的 enqueue_many）
    InitialPrompt,
}

/// 待投递的一条会话消息。队列持久化到 mailbox.json，重启不丢。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedMessage {
    pub id: String,
    #[serde(default)]
    pub from: Option<MessageOrigin>,
    pub body: String,
    pub queued_at: u64,
    #[serde(default)]
    pub kind: MessageKind,
}

/// 群聊活跃度（Clawling 式三档，作用在 agent 互发的入站投递上，参考
/// docs/research/clawling-core-features-crewvela-fit-analysis.md）：
/// Active 正常投递（跟随既有配对限流）；Lurk 收紧到每对 2 条/小时；
/// Muted 暂存 agent 互发不投递（解除后按序补投）。人工 @ 定向（from=None）、
/// 广播与初始任务不受影响——人指挥视为已授权。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AgentActivity {
    #[default]
    Active,
    Lurk,
    Muted,
}

/// 群聊活跃度快照（前端活跃度开关的数据源）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentActivityState {
    pub session_id: String,
    pub activity: AgentActivity,
}

impl QueuedMessage {
    pub fn new(body: String, from: Option<MessageOrigin>) -> Self {
        Self::with_kind(body, from, MessageKind::Chat)
    }

    pub fn with_kind(body: String, from: Option<MessageOrigin>, kind: MessageKind) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from,
            body,
            queued_at: now_nanos(),
            kind,
        }
    }
}

/// `crewvela send` 写入 outbox 的进程间投递格式（跟随 hook 事件的落盘模式）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutboxMessage {
    pub to: String,
    #[serde(default)]
    pub from: Option<MessageOrigin>,
    pub body: String,
}

/// 投递时给 Agent 消息包装来源与回复指引（Orca preamble 的迷你版）；
/// 无来源的消息（广播、初始任务）逐字投递，不加任何包装。
pub fn render_delivery(message: &QueuedMessage) -> String {
    match &message.from {
        Some(origin) => {
            let cli = std::env::current_exe()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|_| "CrewVela".to_string());
            format!("[CrewVela 会话消息｜来自 {}]\n{}\n\n—— 需要回复实质问题时运行（使用发送方原会话 ID；不要对确认、谢谢或结束语反复回信）：\n{}",
                origin.label, message.body, render_reply_commands(&cli, &origin.session_id))
        }
        None => message.body.clone(),
    }
}

fn render_reply_commands(cli: &str, id: &str) -> String {
    let ps_quote = |value: &str| format!("'{}'", value.replace('\'', "''"));
    let bash_quote = |value: &str| format!("'{}'", value.replace('\'', "'\"'\"'"));
    let bash_cli = if cfg!(windows) {
        cli.replace('\\', "/")
    } else {
        cli.to_string()
    };
    format!(
        "PowerShell：& {} send --to {} --body '内容'\nBash：{} send --to {} --body '内容'",
        ps_quote(cli),
        ps_quote(id),
        bash_quote(&bash_cli),
        bash_quote(id)
    )
}

/// 持久任务不固化当前 exe 路径，重启或切换构建后仍保持幂等。
pub fn render_scheduled_delivery(message: &QueuedMessage) -> String {
    match &message.from {
        Some(origin) => format!("[CrewVela 会话消息｜来自 {}]\n{}\n\n回复目标原会话 ID：{}。只在有实质问题或必要协调结果时回复；对收到、谢谢、问候或已完成的确认不要再次回信。\nPowerShell：& $env:CREWVELA_CLI send --to '{}' --body '内容'\nBash：\"$CREWVELA_CLI\" send --to '{}' --body '内容'",
            origin.label, message.body, origin.session_id, origin.session_id.replace('\'', "''"), origin.session_id.replace('\'', "'\"'\"'")),
        None => message.body.clone(),
    }
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        .min(u64::MAX as u128) as u64
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HumanMessageReceipt {
    session_id: String,
    fingerprint: String,
    title: String,
}

fn human_message_fingerprint(session_id: &str, project_id: Option<&str>, body: &str) -> String {
    let tuple = serde_json::to_vec(&(session_id, project_id, body)).expect("string tuple serializes");
    format!("{:x}", Sha256::digest(tuple))
}

#[derive(Serialize, Deserialize)]
struct MailboxFile {
    #[serde(default)]
    handoff_ids: HashSet<String>,
    #[serde(default)]
    processed_outbox_ids: HashSet<String>,
    #[serde(default)]
    human_message_receipts: HashMap<String, HumanMessageReceipt>,
    version: u32,
    items: HashMap<String, VecDeque<QueuedMessage>>,
}

/// 消息文本与提交回车必须分两次写：同 chunk 的 `\r` 会被 ink 按「空输入框上的 Enter」
/// 吞掉（文本留在框里不提交，与 trust 对话框坑 #4 同类）。文本写入后隔 SUBMIT_DELAY 再补 `\r`。
const SUBMIT_DELAY: Duration = Duration::from_millis(600);
/// Agent 互发消息限流：同一对会话（双向合计）滑动窗口内最多 N 条，超限丢弃并回发一次停止通知。
const AGENT_MESSAGE_PAIR_LIMIT: usize = 10;
/// 低活跃（Lurk）会话的收紧配额：只保留低频沟通，防探索性群聊里被高频 agent 刷屏。
const AGENT_MESSAGE_LURK_PAIR_LIMIT: usize = 2;
const AGENT_MESSAGE_PAIR_WINDOW: Duration = Duration::from_secs(3600);
/// 投递记录环形缓冲上限（协作工作台只读投影的数据源）。
const DELIVERY_LOG_CAP: usize = 500;

/// 即时通道的一条成功投递记录：协作工作台「协作记录」的只读投影数据源。
/// 内存环形缓冲，重启清空——即时通道定位是临时对话，不留档案；
/// 需要审计的正式协作应走 multi_agent 持久通道。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryRecord {
    pub id: String,
    /// epoch 毫秒
    pub at: u64,
    pub from_session_id: Option<String>,
    pub from_label: Option<String>,
    pub to_session_id: String,
    pub body: String,
    pub kind: MessageKind,
    pub stage: String,
    pub error: Option<String>,
}

pub struct SessionActionQueue {
    delivery_lock: Mutex<()>,
    persistence_lock: Mutex<()>,
    items: Mutex<HashMap<String, VecDeque<QueuedMessage>>>,
    mode_changes: Mutex<HashMap<String, PendingModeChange>>,
    /// 已写文本、待补提交回车的会话：session_id → 可发 `\r` 的时间点。
    pending_submits: Mutex<HashMap<String, PendingSubmit>>,
    /// agent 互发限流计数：无序会话对 → 窗口内的消息时间戳。内存态，app 重启归零。
    agent_message_log: Mutex<HashMap<(String, String), VecDeque<Instant>>>,
    /// 每对会话每个窗口只回发一次超限通知，避免通知本身再来回。
    agent_message_notices: Mutex<HashMap<(String, String), Instant>>,
    /// 成功投递记录（环形缓冲，封顶 DELIVERY_LOG_CAP 条）。
    delivery_log: Mutex<VecDeque<DeliveryRecord>>,
    delivery_errors: Mutex<HashMap<String, String>>,
    handoff_ids: Mutex<HashSet<String>>,
    processed_outbox_ids: Mutex<HashSet<String>>,
    human_message_receipts: Mutex<HashMap<String, HumanMessageReceipt>>,
    /// 群聊活跃度开关：session_id → AgentActivity。内存态，app 重启归位 Active。
    activity: Mutex<HashMap<String, AgentActivity>>,
    mailbox_path: Option<PathBuf>,
    outbox_dir: Option<PathBuf>,
}

pub struct ControlWorkspaceTrustBatch {
    session_ids: Mutex<Vec<String>>,
}

impl ControlWorkspaceTrustBatch {
    pub fn new() -> Self {
        Self {
            session_ids: Mutex::new(Vec::new()),
        }
    }

    pub fn replace(&self, session_ids: Vec<String>) {
        let mut seen = HashSet::new();
        let unique_ids = session_ids
            .into_iter()
            .filter(|session_id| seen.insert(session_id.clone()))
            .collect();
        *self.session_ids.lock().unwrap() = unique_ids;
    }

    pub fn session_ids(&self) -> Vec<String> {
        self.session_ids.lock().unwrap().clone()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingModeChange {
    Queued(PermissionMode),
    Dispatching {
        target: PermissionMode,
        remaining_steps: usize,
        next_step_at: Instant,
    },
}

impl PendingModeChange {
    fn target(self) -> PermissionMode {
        match self {
            Self::Queued(target) | Self::Dispatching { target, .. } => target,
        }
    }

    fn blocks_prompt(self) -> bool {
        matches!(self, Self::Queued(_) | Self::Dispatching { .. })
    }
}

struct PendingSubmit {
    due: Instant,
    message_id: String,
    pty_id: u32,
    submitted: bool,
    awaiting_prompt_after_blocker: bool,
}

pub struct PermissionModeDispatch {
    pub already_count: usize,
    pub sent_count: usize,
    pub awaiting_confirmation_count: usize,
}

impl SessionActionQueue {
    #[cfg(test)]
    pub fn new() -> Self {
        Self {
            delivery_lock: Mutex::new(()),
            persistence_lock: Mutex::new(()),
            items: Mutex::new(HashMap::new()),
            mode_changes: Mutex::new(HashMap::new()),
            pending_submits: Mutex::new(HashMap::new()),
            agent_message_log: Mutex::new(HashMap::new()),
            agent_message_notices: Mutex::new(HashMap::new()),
            delivery_log: Mutex::new(VecDeque::new()),
            delivery_errors: Mutex::new(HashMap::new()),
            handoff_ids: Mutex::new(HashSet::new()),
            processed_outbox_ids: Mutex::new(HashSet::new()),
            human_message_receipts: Mutex::new(HashMap::new()),
            activity: Mutex::new(HashMap::new()),
            mailbox_path: None,
            outbox_dir: None,
        }
    }

    /// 打开持久化队列：mailbox_path 存待投递消息，outbox_dir 是 CLI `send` 的投递通道。
    pub fn open_at(mailbox_path: PathBuf, outbox_dir: PathBuf) -> Result<Self> {
        if let Some(parent) = mailbox_path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建消息队列目录失败: {}", parent.display()))?;
        }
        std::fs::create_dir_all(&outbox_dir)
            .with_context(|| format!("创建消息 outbox 目录失败: {}", outbox_dir.display()))?;
        let file = match std::fs::read_to_string(&mailbox_path) {
            Ok(raw) => serde_json::from_str::<MailboxFile>(&raw)
                .with_context(|| format!("解析消息队列失败: {}", mailbox_path.display()))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => MailboxFile {
                version: 1,
                items: HashMap::new(),
                handoff_ids: HashSet::new(),
                processed_outbox_ids: HashSet::new(),
                human_message_receipts: HashMap::new(),
            },
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("读取消息队列失败: {}", mailbox_path.display()))
            }
        };
        Ok(Self {
            delivery_lock: Mutex::new(()),
            persistence_lock: Mutex::new(()),
            items: Mutex::new(file.items),
            handoff_ids: Mutex::new(file.handoff_ids),
            processed_outbox_ids: Mutex::new(file.processed_outbox_ids),
            human_message_receipts: Mutex::new(file.human_message_receipts),
            mode_changes: Mutex::new(HashMap::new()),
            pending_submits: Mutex::new(HashMap::new()),
            agent_message_log: Mutex::new(HashMap::new()),
            agent_message_notices: Mutex::new(HashMap::new()),
            delivery_log: Mutex::new(VecDeque::new()),
            delivery_errors: Mutex::new(HashMap::new()),
            activity: Mutex::new(HashMap::new()),
            mailbox_path: Some(mailbox_path),
            outbox_dir: Some(outbox_dir),
        })
    }

    /// 设置会话的群聊活跃度（群聊视图的活跃/低频/静音开关）。
    pub fn set_activity(&self, session_id: &str, activity: AgentActivity) {
        self.activity
            .lock()
            .unwrap()
            .insert(session_id.to_string(), activity);
    }

    pub fn activity(&self, session_id: &str) -> AgentActivity {
        self.activity
            .lock()
            .unwrap()
            .get(session_id)
            .copied()
            .unwrap_or_default()
    }

    /// 批量读取活跃度（入参未出现的会话按默认 Active 返回）。
    pub fn agent_activity_states(&self, session_ids: &[String]) -> Vec<AgentActivityState> {
        let activity = self.activity.lock().unwrap();
        session_ids
            .iter()
            .map(|session_id| AgentActivityState {
                session_id: session_id.clone(),
                activity: activity.get(session_id).copied().unwrap_or_default(),
            })
            .collect()
    }

    /// 目标会话当前的 agent 互发配对限额（Lurk 收紧，Muted 在投递层暂存所以不限流入队）。
    fn pair_limit_for(&self, to: &str) -> usize {
        match self.activity(to) {
            AgentActivity::Lurk => AGENT_MESSAGE_LURK_PAIR_LIMIT,
            AgentActivity::Active | AgentActivity::Muted => AGENT_MESSAGE_PAIR_LIMIT,
        }
    }

    /// 串行持久化当前队列，避免并发调用用旧快照覆盖后来的入队。
    fn persist(&self, _snapshot: &HashMap<String, VecDeque<QueuedMessage>>) {
        let Some(path) = &self.mailbox_path else {
            return;
        };
        let _persistence = self.persistence_lock.lock().unwrap();
        let items = self.items.lock().unwrap().clone();
        let handoff_ids = self.handoff_ids.lock().unwrap().clone();
        let processed_outbox_ids = self.processed_outbox_ids.lock().unwrap().clone();
        let file = MailboxFile { version: 1, items, handoff_ids, processed_outbox_ids, human_message_receipts: self.human_message_receipts.lock().unwrap().clone() };
        if let Err(error) = write_mailbox_file(path, &file) {
            self.delivery_errors
                .lock()
                .unwrap()
                .insert("__mailbox__".into(), format!("消息队列持久化失败：{error}"));
            eprintln!("[crewvela] 保存消息队列失败 {}: {error}", path.display());
        } else {
            self.delivery_errors.lock().unwrap().remove("__mailbox__");
        }
    }

    /// 启动初始任务专用：不进协作记录投影（flush 时按 kind 跳过）。
    pub fn enqueue_many(&self, session_ids: &[String], prompt: String) {
        let snapshot =
            {
                let mut items = self.items.lock().unwrap();
                for session_id in session_ids {
                    items.entry(session_id.clone()).or_default().push_back(
                        QueuedMessage::with_kind(prompt.clone(), None, MessageKind::InitialPrompt),
                    );
                }
                items.clone()
            };
        self.persist(&snapshot);
    }

    pub fn enqueue_message(&self, session_id: &str, message: QueuedMessage) {
        let snapshot = {
            let mut items = self.items.lock().unwrap();
            items
                .entry(session_id.to_string())
                .or_default()
                .push_back(message);
            items.clone()
        };
        self.persist(&snapshot);
    }

    /// Fallible entry for direct collaboration and automation. Publish the whole
    /// batch after persistence, so an error can never leave a partial delivery.
    pub(crate) fn enqueue_messages_durable(&self, deliveries: Vec<(String, QueuedMessage)>) -> Result<()> {
        let _persistence = self.persistence_lock.lock().unwrap();
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        for (session_id, message) in deliveries {
            next.entry(session_id).or_default().push_back(message);
        }
        if let Some(path) = &self.mailbox_path {
            if let Err(error) = write_mailbox_file(path, &MailboxFile {
                version: 1, items: next.clone(),
                handoff_ids: self.handoff_ids.lock().unwrap().clone(),
                processed_outbox_ids: self.processed_outbox_ids.lock().unwrap().clone(),
                human_message_receipts: self.human_message_receipts.lock().unwrap().clone(),
            }) {
                let error = error.context("消息未入队，持久化失败");
                self.delivery_errors.lock().unwrap().insert("__mailbox__".into(), format!("{error:#}"));
                return Err(error);
            }
        }
        *items = next;
        self.delivery_errors.lock().unwrap().remove("__mailbox__");
        Ok(())
    }

    /// 查看队首消息但不消费；flush_ready 在 PTY 写成功后才 commit_next，
    /// 写失败消息不丢、顺序不乱。
    pub(crate) fn has_pending_work(&self, session_id: &str) -> bool {
        let queued = self.items.lock().unwrap().get(session_id).is_some_and(|items| !items.is_empty());
        queued || self.pending_submits.lock().unwrap().contains_key(session_id)
    }

    #[cfg(test)]
    pub fn peek_next(&self, session_id: &str) -> Option<QueuedMessage> {
        self.items.lock().unwrap().get(session_id)?.front().cloned()
    }

    /// 静音只挂起 Agent Chat，人工插话可越过它；Agent Chat 之间顺序不变。
    pub(crate) fn peek_deliverable(&self, session_id: &str) -> Option<QueuedMessage> {
        let muted = self.activity(session_id) == AgentActivity::Muted;
        self.items
            .lock()
            .unwrap()
            .get(session_id)?
            .iter()
            .find(|message| !muted || message.kind != MessageKind::Chat || message.from.is_none())
            .cloned()
    }

    pub(crate) fn commit_delivered_message(&self, session_id: &str, message_id: &str) -> bool {
        match self.try_commit_delivered_message(session_id, message_id) {
            Ok(committed) => committed,
            Err(error) => {
                self.delivery_errors.lock().unwrap().insert("__mailbox__".into(), format!("消息提交确认持久化失败：{error:#}"));
                false
            }
        }
    }

    fn try_commit_delivered_message(&self, session_id: &str, message_id: &str) -> Result<bool> {
        let _persistence = self.persistence_lock.lock().unwrap();
        let handoff_ids = self.handoff_ids.lock().unwrap().clone();
        let processed_outbox_ids = self.processed_outbox_ids.lock().unwrap().clone();
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let Some(queue) = next.get_mut(session_id) else { return Ok(false); };
        let Some(index) = queue.iter().position(|message| message.id == message_id) else { return Ok(false); };
        queue.remove(index);
        if queue.is_empty() { next.remove(session_id); }
        if let Some(path) = &self.mailbox_path {
            write_mailbox_file(path, &MailboxFile {
                version: 1, items: next.clone(), handoff_ids, processed_outbox_ids,
                human_message_receipts: self.human_message_receipts.lock().unwrap().clone(),
            })?;
        }
        *items = next;
        self.delivery_errors.lock().unwrap().remove("__mailbox__");
        Ok(true)
    }

    fn enqueue_outbox_message(&self, session_id: &str, message: QueuedMessage) -> Result<bool> {
        let path = self.mailbox_path.as_ref().context("消息队列没有持久化路径")?;
        let _persistence = self.persistence_lock.lock().unwrap();
        let handoff_ids = self.handoff_ids.lock().unwrap().clone();
        let mut items = self.items.lock().unwrap();
        let mut processed = self.processed_outbox_ids.lock().unwrap();
        if processed.contains(&message.id) { return Ok(false); }
        let mut next_items = items.clone();
        let mut next_processed = processed.clone();
        next_processed.insert(message.id.clone());
        next_items.entry(session_id.into()).or_default().push_back(message);
        let result = write_mailbox_file(path, &MailboxFile {
            version: 1, items: next_items.clone(), handoff_ids,
            processed_outbox_ids: next_processed.clone(),
            human_message_receipts: self.human_message_receipts.lock().unwrap().clone(),
        });
        if let Err(error) = result {
            self.delivery_errors.lock().unwrap().insert("__mailbox__".into(), format!("消息入队持久化失败：{error:#}"));
            return Err(error);
        }
        *items = next_items;
        *processed = next_processed;
        self.delivery_errors.lock().unwrap().remove("__mailbox__");
        Ok(true)
    }

    /// A client request can be retried after losing its IPC reply, even after delivery.
    pub(crate) fn accepted_human_message(
        &self,
        request_id: &str,
        session_id: &str,
        project_id: Option<&str>,
        body: &str,
    ) -> Result<Option<String>> {
        let receipts = self.human_message_receipts.lock().unwrap();
        match receipts.get(request_id) {
            Some(receipt) if receipt.fingerprint == human_message_fingerprint(session_id, project_id, body) => Ok(Some(receipt.title.clone())),
            Some(_) => Err(anyhow!("消息请求 ID 已用于其他收件人或内容，请重新发送新消息")),
            None => Ok(None),
        }
    }

    /// Permanent deletion ends the lifetime of its retry receipts. Archive/stop do not.
    pub(crate) fn forget_human_message_receipts(&self, session_id: &str) -> Result<()> {
        let _persistence = self.persistence_lock.lock().unwrap();
        let mut receipts = self.human_message_receipts.lock().unwrap();
        if !receipts.values().any(|receipt| receipt.session_id == session_id) { return Ok(()); }
        let mut next = receipts.clone();
        next.retain(|_, receipt| receipt.session_id != session_id);
        let path = self.mailbox_path.as_ref().context("消息队列没有持久化路径")?;
        write_mailbox_file(path, &MailboxFile {
            version: 1, items: self.items.lock().unwrap().clone(),
            handoff_ids: self.handoff_ids.lock().unwrap().clone(),
            processed_outbox_ids: self.processed_outbox_ids.lock().unwrap().clone(),
            human_message_receipts: next.clone(),
        }).context("会话已删除，但人工消息回执清理失败")?;
        *receipts = next;
        Ok(())
    }

    /// Publish to the live queue only after both the message and retry receipt are durable.
    /// A failed write leaves no deliverable message and permits a safe retry.
    pub(crate) fn enqueue_human_message(
        &self,
        session_id: &str,
        project_id: Option<&str>,
        title: &str,
        message: QueuedMessage,
        remember_request: bool,
    ) -> Result<()> {
        let path = self.mailbox_path.as_ref().context("消息队列没有持久化路径")?;
        let _persistence = self.persistence_lock.lock().unwrap();
        let mut items = self.items.lock().unwrap();
        let mut receipts = self.human_message_receipts.lock().unwrap();
        let fingerprint = human_message_fingerprint(session_id, project_id, &message.body);
        if let Some(receipt) = receipts.get(&message.id) {
            if receipt.fingerprint != fingerprint {
                return Err(anyhow!("消息请求 ID 已用于其他收件人或内容，请重新发送新消息"));
            }
            return Ok(());
        }
        let mut next = items.clone();
        let mut next_receipts = receipts.clone();
        if remember_request {
            next_receipts.insert(message.id.clone(), HumanMessageReceipt {
                session_id: session_id.to_string(), fingerprint, title: title.to_string(),
            });
        }
        next.entry(session_id.to_string()).or_default().push_back(message);
        let result = write_mailbox_file(path, &MailboxFile {
            version: 1, items: next.clone(),
            handoff_ids: self.handoff_ids.lock().unwrap().clone(),
            processed_outbox_ids: self.processed_outbox_ids.lock().unwrap().clone(),
            human_message_receipts: next_receipts.clone(),
        });
        if let Err(error) = result {
            self.delivery_errors.lock().unwrap().insert("__mailbox__".into(), format!("人工消息未入队，持久化失败：{error:#}"));
            return Err(error);
        }
        *items = next;
        *receipts = next_receipts;
        self.delivery_errors.lock().unwrap().remove("__mailbox__");
        Ok(())
    }

    /// 确认消费队首消息（PTY 写成功后调用）并落盘。
    #[cfg(test)]
    pub fn commit_next(&self, session_id: &str) {
        let snapshot = {
            let mut items = self.items.lock().unwrap();
            if let Some(queue) = items.get_mut(session_id) {
                queue.pop_front();
                if queue.is_empty() {
                    items.remove(session_id);
                }
            }
            items.clone()
        };
        self.persist(&snapshot);
    }

    /// 幂等桥接：并发 poll 只能确认它实际落库的那一条，不消费后继。
    #[cfg(test)]
    pub fn commit_if_id_matches(&self, session_id: &str, message_id: &str) -> bool {
        let snapshot = {
            let mut items = self.items.lock().unwrap();
            let Some(queue) = items.get_mut(session_id) else {
                return false;
            };
            if !queue
                .front()
                .is_some_and(|message| message.id == message_id)
            {
                return false;
            }
            queue.pop_front();
            if queue.is_empty() {
                items.remove(session_id);
            }
            items.clone()
        };
        self.persist(&snapshot);
        true
    }

    /// Called after a user's Enter was successfully written to this terminal.
    /// Cancel only the automatic Enter, and durably acknowledge the same message.
    pub fn cancel_pending_submit(&self, session_id: &str) {
        let _delivery = self.delivery_lock.lock().unwrap();
        let mut pending = self.pending_submits.lock().unwrap();
        if let Some(submit) = pending.get_mut(session_id) {
            submit.submitted = true;
            if self.acknowledge_submitted_message(session_id, &submit.message_id) { pending.remove(session_id); }
        }
    }

    /// An Enter sent to a permission/input prompt is not a message submission.
    /// Record the blocker before the caller restores Running, even when no
    /// automatic flush observed the short-lived Waiting status.
    pub fn hold_pending_submit_for_blocker(&self, session_id: &str) {
        let _delivery = self.delivery_lock.lock().unwrap();
        if let Some(submit) = self.pending_submits.lock().unwrap().get_mut(session_id) {
            if !submit.submitted {
                submit.awaiting_prompt_after_blocker = true;
                self.update_delivery_stage(session_id, "text_written", Some("目标正在等待人工处理，消息尚未提交；不会自动确认权限或输入提示".into()));
            }
        }
    }

    fn acknowledge_submitted_message(&self, session_id: &str, message_id: &str) -> bool {
        match self.try_commit_delivered_message(session_id, message_id) {
            Ok(_) => { self.update_delivery_stage(session_id, "submit_sent", None); true }
            Err(error) => {
                let error = format!("提交回车已发送，但确认落盘失败；重启前请核对目标以免重复：{error:#}");
                self.delivery_errors.lock().unwrap().insert("__mailbox__".into(), error.clone());
                self.update_delivery_stage(session_id, "submit_sent", Some(error));
                false
            }
        }
    }

    #[cfg(test)]
    pub fn take_next(&self, session_id: &str) -> Option<QueuedMessage> {
        let (message, snapshot) = {
            let mut items = self.items.lock().unwrap();
            let message = items.get_mut(session_id)?.pop_front();
            if items
                .get(session_id)
                .is_some_and(|prompts| prompts.is_empty())
            {
                items.remove(session_id);
            }
            (message, items.clone())
        };
        self.persist(&snapshot);
        message
    }

    fn record_runtime_switch_failure(&self, session_id: &str, message: QueuedMessage) {
        let mut log = self.delivery_log.lock().unwrap();
        if log.len() >= DELIVERY_LOG_CAP {
            log.pop_front();
        }
        log.push_back(DeliveryRecord {
            id: message.id,
            at: now_nanos() / 1_000_000,
            from_session_id: message
                .from
                .as_ref()
                .map(|origin| origin.session_id.clone()),
            from_label: message.from.as_ref().map(|origin| origin.label.clone()),
            to_session_id: session_id.into(),
            body: message.body,
            kind: message.kind,
            stage: "failed".into(),
            error: Some("收发会话已因 Runtime 切换退役，消息未投递；请重新发送到当前会话".into()),
        });
    }

    pub fn fail_retired_session(&self, session_id: &str) {
        let messages = self
            .items
            .lock()
            .unwrap()
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        for message in messages {
            self.record_runtime_switch_failure(session_id, message);
        }
        self.discard(session_id);
    }

    pub fn discard(&self, session_id: &str) {
        let snapshot = {
            let mut items = self.items.lock().unwrap();
            items.remove(session_id);
            items.clone()
        };
        self.mode_changes.lock().unwrap().remove(session_id);
        self.pending_submits.lock().unwrap().remove(session_id);
        self.activity.lock().unwrap().remove(session_id);
        self.persist(&snapshot);
    }

    pub fn clear(&self) {
        let snapshot = {
            let mut items = self.items.lock().unwrap();
            items.clear();
            items.clone()
        };
        self.mode_changes.lock().unwrap().clear();
        self.pending_submits.lock().unwrap().clear();
        self.activity.lock().unwrap().clear();
        self.persist(&snapshot);
    }

    /// 摄取 CLI `send` 写入 outbox 的进程外消息，返回入队条数。
    /// 目标会话不存在的消息丢弃并记日志，避免孤儿消息永远滞留。
    #[cfg(test)]
    pub fn ingest_outbox(
        &self,
        session_state: &SessionManager,
        mut on_ingested: impl FnMut(&OutboxMessage) -> Result<()>,
    ) -> Result<usize> {
        self.ingest_outbox_with_identity(session_state, |message, _| on_ingested(message))
    }

    pub fn ingest_outbox_with_identity(
        &self,
        session_state: &SessionManager,
        mut on_ingested: impl FnMut(&OutboxMessage, &str) -> Result<()>,
    ) -> Result<usize> {
        let _delivery = self.delivery_lock.lock().unwrap();
        let Some(outbox_dir) = &self.outbox_dir else {
            return Ok(0);
        };
        let mut paths = std::fs::read_dir(outbox_dir)
            .with_context(|| format!("读取消息 outbox 目录失败: {}", outbox_dir.display()))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "json")
            })
            .collect::<Vec<_>>();
        paths.sort();
        // Only retain receipts for source files which still exist (e.g. a failed
        // cleanup). This bounds durable deduplication without reopening a replay
        // window while an unremoved outbox source can still be retried.
        let present_ids = paths.iter().map(|path| outbox_message_id(path)).collect::<Result<HashSet<_>>>()?;
        let removed_receipts = {
            let mut processed = self.processed_outbox_ids.lock().unwrap();
            let before = processed.len();
            processed.retain(|id| present_ids.contains(id));
            processed.len() != before
        };
        if removed_receipts { self.persist(&HashMap::new()); }

        let mut ingested = 0;
        let mut persistence_error = None;
        for path in paths {
            let mut persistence_failed = false;
            let outcome = (|| -> Result<bool> {
                let message_id = outbox_message_id(&path)?;
                if self.processed_outbox_ids.lock().unwrap().contains(&message_id) { return Ok(false); }
                let raw = std::fs::read_to_string(&path)?;
                let message: OutboxMessage = serde_json::from_str(&raw)?;
                let Some(target) = session_state.get(&message.to) else {
                    eprintln!("[crewvela] 丢弃目标不存在的会话消息 to={}", message.to);
                    return Ok(false);
                };
                if target.retired_agent_runtime
                    || message.from.as_ref().is_some_and(|origin| {
                        session_state
                            .get(&origin.session_id)
                            .is_some_and(|sender| sender.retired_agent_runtime)
                    })
                {
                    self.record_runtime_switch_failure(
                        &message.to,
                        QueuedMessage::new(message.body.clone(), message.from.clone()),
                    );
                    return Ok(false);
                }
                crate::runtime_preferences::ensure_session_available(&target)?;
                if let Some(sender) = message
                    .from
                    .as_ref()
                    .and_then(|origin| session_state.get(&origin.session_id))
                {
                    crate::runtime_preferences::ensure_session_available(&sender)?;
                }
                // agent 互发限流：只统计有来源的消息（广播/初始任务 from=None 不计）。
                if let Some(origin) = &message.from {
                    if !self.agent_message_quota_available(
                        &origin.session_id,
                        &message.to,
                        Instant::now(),
                    ) {
                        self.enqueue_agent_limit_notice(
                            &origin.session_id,
                            &message.to,
                            &target.title,
                            self.pair_limit_for(&message.to),
                            Instant::now(),
                        );
                        return Ok(false);
                    }
                }
                let mut queued = QueuedMessage::new(message.body.clone(), message.from.clone());
                queued.id = message_id;
                on_ingested(&message, &queued.id)?;
                let added = self.enqueue_outbox_message(&message.to, queued).map_err(|error| {
                    persistence_failed = true;
                    error
                })?;
                if added {
                    if let Some(origin) = &message.from {
                        // Failed callbacks or disk writes never consume quota.
                        self.record_agent_message_at(&origin.session_id, &message.to, Instant::now());
                    }
                }
                Ok(added)
            })();
            let should_remove = outcome.is_ok();
            match outcome {
                Ok(true) => ingested += 1,
                Ok(false) => {}
                Err(error) => {
                    eprintln!(
                        "[crewvela] 摄取会话消息失败 {}: {error:#}",
                        path.display()
                    );
                    if persistence_failed && persistence_error.is_none() { persistence_error = Some(error); }
                }
            }
            if should_remove {
                if let Err(error) = std::fs::remove_file(&path) {
                    eprintln!("[crewvela] 清理会话消息失败 {}: {error}", path.display());
                }
            }
        }
        if let Some(error) = persistence_error { return Err(error).context("会话消息尚未持久化，outbox 源文件已保留"); }
        Ok(ingested)
    }

    fn agent_message_quota_available(&self, from: &str, to: &str, now: Instant) -> bool {
        let limit = self.pair_limit_for(to);
        let mut log = self.agent_message_log.lock().unwrap();
        let entries = log.entry(agent_pair_key(from, to)).or_default();
        while entries.front().is_some_and(|at| now.duration_since(*at) >= AGENT_MESSAGE_PAIR_WINDOW) { entries.pop_front(); }
        entries.len() < limit
    }

    /// 记录一对会话间的 agent 消息（滑动窗口去旧）；返回 false 表示已达上限，消息应丢弃。
    /// 限额按目标会话的活跃度取值（Lurk 收紧）。
    fn record_agent_message_at(&self, from: &str, to: &str, now: Instant) -> bool {
        let limit = self.pair_limit_for(to);
        let key = agent_pair_key(from, to);
        let mut log = self.agent_message_log.lock().unwrap();
        let entries = log.entry(key).or_default();
        while entries
            .front()
            .is_some_and(|at| now.duration_since(*at) >= AGENT_MESSAGE_PAIR_WINDOW)
        {
            entries.pop_front();
        }
        if entries.len() >= limit {
            return false;
        }
        entries.push_back(now);
        true
    }

    /// 超限后给发送方回发停止通知（from=None 不计入限流；每对会话每窗口只发一次，
    /// 否则「通知 → agent 回复 → 又超限 → 又通知」会自己形成烧 token 回路）。
    fn enqueue_agent_limit_notice(
        &self,
        from: &str,
        to: &str,
        target_title: &str,
        limit: usize,
        now: Instant,
    ) {
        let key = agent_pair_key(from, to);
        {
            let mut notices = self.agent_message_notices.lock().unwrap();
            if notices
                .get(&key)
                .is_some_and(|at| now.duration_since(*at) < AGENT_MESSAGE_PAIR_WINDOW)
            {
                return;
            }
            notices.insert(key, now);
        }
        self.enqueue_message(
            from,
            QueuedMessage::with_kind(
                format!(
                    "[CrewVela 系统消息] 你与「{target_title}」之间的会话消息已达上限\
                    （{limit} 条/小时），刚才那条未投递。请总结当前进展并停止互发；\
                    确需继续请等待窗口重置或由人工转达。"
                ),
                None,
                MessageKind::System,
            ),
        );
    }

    /// 人工定向指挥某个会话时，豁免它参与的所有会话对的限流配额与通知状态：
    /// 人在场指挥等于已知情，不需要保护；纯 agent 自发互发仍受限。
    pub fn reset_agent_quota_involving(&self, session_id: &str) {
        self.agent_message_log
            .lock()
            .unwrap()
            .retain(|(a, b), _| a != session_id && b != session_id);
        self.agent_message_notices
            .lock()
            .unwrap()
            .retain(|(a, b), _| a != session_id && b != session_id);
    }

    pub fn enqueue_permission_mode(
        &self,
        target_mode: PermissionMode,
        sessions: &[SessionRecord],
    ) -> Result<PermissionModeReceipt> {
        if !target_mode.is_unified_target() {
            return Err(anyhow!(
                "统一模式只支持 Default、Accept Edits、Plan 和 Auto"
            ));
        }
        let targets = sessions
            .iter()
            .filter(|session| {
                session.kind == SessionKind::Claude
                    && session.pty_id.is_some()
                    && crate::runtime_preferences::ensure_session_available(session).is_ok()
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return Err(anyhow!("当前没有可切换模式的运行会话"));
        }
        for session in &targets {
            let current = session
                .permission_mode
                .ok_or_else(|| anyhow!("会话“{}”尚未上报当前 Mode，请稍后重试", session.title))?;
            if permission_mode_steps(current, target_mode, session.yolo).is_none() {
                return Err(anyhow!(
                    "会话“{}”当前处于 {:?}，无法可靠切换到目标 Mode",
                    session.title,
                    current
                ));
            }
        }

        let mut mode_changes = self.mode_changes.lock().unwrap();
        if targets.iter().any(|session| {
            mode_changes
                .get(&session.id)
                .is_some_and(|change| change.target() != target_mode)
        }) {
            return Err(anyhow!("已有统一模式切换正在执行，请稍后再选择其他模式"));
        }
        for session in targets.iter() {
            mode_changes
                .entry(session.id.clone())
                .or_insert(PendingModeChange::Queued(target_mode));
        }
        Ok(PermissionModeReceipt {
            target_mode,
            target_count: targets.len(),
            already_count: 0,
            sent_count: 0,
            awaiting_confirmation_count: 0,
            queued_count: targets.len(),
        })
    }

    pub fn flush_permission_modes(
        &self,
        session_state: &SessionManager,
        pty_state: &PtyManager,
    ) -> PermissionModeDispatch {
        self.flush_permission_modes_with_writer_at(
            session_state,
            Instant::now(),
            |pty_id, bytes| pty_state.write(pty_id, bytes),
        )
    }

    fn flush_permission_modes_with_writer_at<F>(
        &self,
        session_state: &SessionManager,
        now: Instant,
        mut write: F,
    ) -> PermissionModeDispatch
    where
        F: FnMut(u32, &[u8]) -> Result<()>,
    {
        let mut dispatch = PermissionModeDispatch {
            already_count: 0,
            sent_count: 0,
            awaiting_confirmation_count: 0,
        };
        let sessions = session_state.list();
        let mut mode_changes = self.mode_changes.lock().unwrap();
        for session in sessions {
            if crate::runtime_preferences::ensure_session_available(&session).is_err() {
                continue;
            }
            let Some(pty_id) = session.pty_id else {
                continue;
            };
            let Some(change) = mode_changes.get(&session.id).copied() else {
                continue;
            };
            let Some(current_mode) = session.permission_mode else {
                continue;
            };
            let target_mode = change.target();
            if current_mode == target_mode {
                mode_changes.remove(&session.id);
                dispatch.already_count += 1;
                continue;
            }

            match change {
                PendingModeChange::Dispatching {
                    remaining_steps,
                    next_step_at,
                    ..
                } => {
                    if now < next_step_at || !session_state.is_ready_for_prompt(&session.id) {
                        continue;
                    }
                    if write(pty_id, BACKTAB).is_err() {
                        continue;
                    }
                    dispatch.sent_count += 1;
                    let next_change = if remaining_steps > 1 {
                        Some(PendingModeChange::Dispatching {
                            target: target_mode,
                            remaining_steps: remaining_steps - 1,
                            next_step_at: now + MODE_STEP_DELAY,
                        })
                    } else {
                        match session_state.predict_permission_mode_if_revision(
                            &session.id,
                            target_mode,
                            session.permission_mode_revision,
                        ) {
                            Ok(true) => None,
                            _ => Some(PendingModeChange::Queued(target_mode)),
                        }
                    };
                    if let Some(next_change) = next_change {
                        mode_changes.insert(session.id.clone(), next_change);
                    } else {
                        mode_changes.remove(&session.id);
                    }
                    continue;
                }
                _ => {}
            }

            if session.runtime_command_host
                || self
                    .pending_submits
                    .lock()
                    .unwrap()
                    .contains_key(&session.id)
            {
                continue;
            }
            if !session_state.is_ready_for_prompt(&session.id) {
                mode_changes.insert(session.id.clone(), PendingModeChange::Queued(target_mode));
                continue;
            }
            let Some(steps) = permission_mode_steps(current_mode, target_mode, session.yolo) else {
                mode_changes.insert(session.id.clone(), PendingModeChange::Queued(target_mode));
                continue;
            };
            if steps > 0 {
                if write(pty_id, BACKTAB).is_err() {
                    mode_changes.insert(session.id.clone(), PendingModeChange::Queued(target_mode));
                    continue;
                }
                dispatch.sent_count += 1;
                let next_change = if steps > 1 {
                    Some(PendingModeChange::Dispatching {
                        target: target_mode,
                        remaining_steps: steps - 1,
                        next_step_at: now + MODE_STEP_DELAY,
                    })
                } else {
                    match session_state.predict_permission_mode_if_revision(
                        &session.id,
                        target_mode,
                        session.permission_mode_revision,
                    ) {
                        Ok(true) => None,
                        _ => Some(PendingModeChange::Queued(target_mode)),
                    }
                };
                if let Some(next_change) = next_change {
                    mode_changes.insert(session.id.clone(), next_change);
                } else {
                    mode_changes.remove(&session.id);
                }
            }
        }
        dispatch
    }

    #[cfg(test)]
    fn pending_mode(&self, session_id: &str) -> Option<PermissionMode> {
        self.mode_changes
            .lock()
            .unwrap()
            .get(session_id)
            .copied()
            .map(PendingModeChange::target)
    }

    fn mode_change_blocks_prompt(&self, session_id: &str) -> bool {
        self.mode_changes
            .lock()
            .unwrap()
            .get(session_id)
            .is_some_and(|change| change.blocks_prompt())
    }

    pub fn enqueue_broadcast(
        &self,
        prompt: &str,
        sessions: &[SessionRecord],
    ) -> Result<BroadcastReceipt> {
        if prompt.trim().is_empty() {
            return Err(anyhow!("广播 Prompt 不能为空"));
        }
        if prompt.chars().count() > 32_000 {
            return Err(anyhow!("广播 Prompt 不能超过 32000 个字符"));
        }
        let targets = sessions
            .iter()
            .filter(|session| {
                session.kind == SessionKind::Claude
                    && session.pty_id.is_some()
                    && crate::runtime_preferences::ensure_session_available(session).is_ok()
            })
            .map(|session| BroadcastTarget {
                session_id: session.id.clone(),
                title: session.title.clone(),
                provider_name: session.provider_name.clone(),
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return Err(anyhow!("当前没有可广播的运行会话"));
        }
        let _persistence = self.persistence_lock.lock().unwrap();
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        for target in &targets {
            next.entry(target.session_id.clone())
                .or_default()
                .push_back(QueuedMessage::with_kind(
                    prompt.to_string(),
                    None,
                    MessageKind::Broadcast,
                ));
        }
        if let Some(path) = &self.mailbox_path {
            if let Err(error) = write_mailbox_file(path, &MailboxFile {
                version: 1,
                items: next.clone(),
                handoff_ids: self.handoff_ids.lock().unwrap().clone(),
                processed_outbox_ids: self.processed_outbox_ids.lock().unwrap().clone(),
                human_message_receipts: self.human_message_receipts.lock().unwrap().clone(),
            }) {
                self.delivery_errors.lock().unwrap().insert("__mailbox__".into(), format!("广播未入队，持久化失败：{error:#}"));
                return Err(error).context("广播未入队，消息队列持久化失败");
            }
        }
        *items = next;
        self.delivery_errors.lock().unwrap().remove("__mailbox__");
        Ok(BroadcastReceipt {
            target_count: targets.len(),
            targets,
        })
    }

    pub fn flush_ready(
        &self,
        session_state: &SessionManager,
        pty_state: &PtyManager,
    ) -> Vec<String> {
        self.flush_ready_with_writer_at(session_state, Instant::now(), |pty_id, bytes| {
            pty_state.write(pty_id, bytes)
        })
    }

    fn flush_ready_with_writer_at<F>(
        &self,
        session_state: &SessionManager,
        now: Instant,
        mut write: F,
    ) -> Vec<String>
    where
        F: FnMut(u32, &[u8]) -> Result<()>,
    {
        let _delivery = self.delivery_lock.lock().unwrap();
        let mut dispatched = Vec::new();
        for session in session_state.list() {
            if let Err(error) = crate::runtime_preferences::ensure_session_available(&session) {
                self.delivery_errors
                    .lock()
                    .unwrap()
                    .insert(session.id.clone(), error.to_string());
                continue;
            }
            let Some(pty_id) = session.pty_id else {
                continue;
            };
            if session.runtime_command_host
                || self
                    .pending_submits
                    .lock()
                    .unwrap()
                    .contains_key(&session.id)
            {
                continue;
            }
            if matches!(session.agent_runtime_kind.as_deref(), Some("pi")) {
                if let Some(message) = self.peek_deliverable(&session.id) {
                    if session_state.runtime_message_accepted(&session.id, &message.id) {
                        if !self.commit_delivered_message(&session.id, &message.id) { continue; }
                        if message.kind != MessageKind::InitialPrompt {
                            self.record_delivery(&session.id, &message);
                            self.update_delivery_stage(&session.id, "bridge_received", None);
                        }
                        dispatched.push(session.id.clone());
                        continue;
                    }
                }
            }
            if !matches!(session.agent_runtime_kind.as_deref(), Some("pi"))
                && !session_state.is_ready_for_prompt(&session.id)
            {
                continue;
            }
            if self.mode_change_blocks_prompt(&session.id) {
                continue;
            }
            let Some(message) = self.peek_deliverable(&session.id) else {
                continue;
            };
            // 静音的会话暂存 agent 互发（带来源的 Chat），解除后按序补投；
            // 人工 @ 定向（from=None）、广播与初始任务照常投递。
            if message.kind == MessageKind::Chat
                && message.from.is_some()
                && self.activity(&session.id) == AgentActivity::Muted
            {
                continue;
            }
            // 只写文本不带 \r；提交回车由 flush_pending_submits 延迟补发（见 SUBMIT_DELAY）。
            let payload = render_delivery(&message);
            if matches!(session.agent_runtime_kind.as_deref(), Some("pi")) {
                match session_state.queue_runtime_delivery(&session.id, &message.id, &payload) {
                    Ok(true) => session_state.mark_prompt_dispatched(&session.id),
                    Ok(false) => {}
                    Err(error) => {
                        self.delivery_errors.lock().unwrap().insert(
                            session.id.clone(),
                            format!("Runtime 消息收件箱写入失败: {error:#}"),
                        );
                    }
                }
                // Remain in the original mailbox until Pi's session store confirms receipt.
                continue;
            }
            let safe_text = payload
                .chars()
                .filter(|character| !character.is_control() || matches!(character, '\n' | '\t'))
                .collect::<String>();
            let paste = format!("\x1b[200~{safe_text}\x1b[201~");
            match write(pty_id, paste.as_bytes()) {
                Ok(()) => {
                    self.pending_submits
                        .lock()
                        .unwrap()
                        .insert(session.id.clone(), PendingSubmit {
                            due: now + SUBMIT_DELAY, message_id: message.id.clone(),
                            pty_id, submitted: false, awaiting_prompt_after_blocker: false,
                        });
                    // 初始任务是启动流程不是协作，不进协作记录投影。
                    if message.kind != MessageKind::InitialPrompt {
                        self.record_delivery(&session.id, &message);
                    }
                    session_state.mark_prompt_dispatched(&session.id);
                    dispatched.push(session.id);
                }
                // 写失败：消息留在队首，下一轮 poll 重试，不丢不重排。
                Err(error) => {
                    self.delivery_errors
                        .lock()
                        .unwrap()
                        .insert(session.id.clone(), format!("消息文本写入失败: {error:#}"));
                }
            }
        }
        dispatched
    }

    /// 记录一条成功投递（环形缓冲，满则挤出最旧）。body 截断防长消息撑爆内存。
    fn record_delivery(&self, to_session_id: &str, message: &QueuedMessage) {
        self.delivery_errors.lock().unwrap().remove(to_session_id);
        let mut log = self.delivery_log.lock().unwrap();
        if log.len() >= DELIVERY_LOG_CAP {
            log.pop_front();
        }
        log.push_back(DeliveryRecord {
            id: message.id.clone(),
            at: now_nanos() / 1_000_000,
            from_session_id: message
                .from
                .as_ref()
                .map(|origin| origin.session_id.clone()),
            from_label: message.from.as_ref().map(|origin| origin.label.clone()),
            to_session_id: to_session_id.to_string(),
            body: message.body.chars().take(2_000).collect(),
            kind: message.kind,
            stage: "text_written".into(),
            error: None,
        });
    }

    /// 全部成功投递记录，协作工作台「协作记录」的只读投影数据源。
    pub fn delivery_records(&self) -> Vec<DeliveryRecord> {
        self.delivery_log.lock().unwrap().iter().cloned().collect()
    }

    pub(crate) fn communication_evidence(&self) -> Vec<serde_json::Value> {
        let mut result: Vec<_> = self
            .delivery_records()
            .into_iter()
            .map(|record| serde_json::json!(record))
            .collect();
        let logged_ids = result.iter().filter_map(|entry| entry.get("id").and_then(serde_json::Value::as_str)).map(str::to_owned).collect::<HashSet<_>>();
        for (session, messages) in self.items.lock().unwrap().iter() {
            for message in messages {
                if logged_ids.contains(&message.id) { continue; }
                let error = {
                    let errors = self.delivery_errors.lock().unwrap();
                    errors
                        .get(session)
                        .or_else(|| errors.get("__mailbox__"))
                        .cloned()
                };
                result.push(serde_json::json!({"id": message.id, "toSessionId": session,
                    "at": message.queued_at / 1_000_000,
                    "fromSessionId": message.from.as_ref().map(|origin| &origin.session_id),
                    "fromLabel": message.from.as_ref().map(|origin| &origin.label),
                    "kind": message.kind,
                    "body": message.body,
                    "stage": "queued", "error": error}));
            }
        }
        result
    }

    pub(crate) fn mailbox_error(&self) -> Option<String> {
        self.delivery_errors
            .lock()
            .unwrap()
            .get("__mailbox__")
            .cloned()
    }

    pub(crate) fn handoff_enqueued(&self, id: &str) -> bool {
        self.handoff_ids.lock().unwrap().contains(id)
    }

    pub(crate) fn suspend_delivery_for_handoff<T>(
        &self,
        session_id: &str,
        stop: impl FnOnce() -> Result<T>,
    ) -> Result<T> {
        let _delivery = self.delivery_lock.lock().unwrap();
        if self
            .pending_submits
            .lock()
            .unwrap()
            .contains_key(session_id)
        {
            return Err(anyhow!(
                "原会话仍有文本等待提交回车，请等待投递完成再交接；未停止原会话"
            ));
        }
        stop()
    }

    fn update_delivery_stage(&self, session_id: &str, stage: &str, error: Option<String>) {
        if let Some(record) = self
            .delivery_log
            .lock()
            .unwrap()
            .iter_mut()
            .rev()
            .find(|record| record.to_session_id == session_id && record.stage == "text_written")
        {
            record.stage = stage.to_string();
            record.error = error;
        }
    }

    /// 保存失败时不入内存队列，调用方保留持久化交接草稿重试。
    pub(crate) fn enqueue_handoff(&self, session_id: &str, message: QueuedMessage) -> Result<()> {
        let _delivery = self.delivery_lock.lock().unwrap();
        if self.handoff_ids.lock().unwrap().contains(&message.id)
            || self
                .items
                .lock()
                .unwrap()
                .values()
                .any(|messages| messages.iter().any(|item| item.id == message.id))
            || self
                .delivery_log
                .lock()
                .unwrap()
                .iter()
                .any(|item| item.id == message.id)
        {
            return Ok(());
        }
        let path = self
            .mailbox_path
            .as_ref()
            .context("消息队列没有持久化路径，不能交接")?;
        let _persistence = self.persistence_lock.lock().unwrap();
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let mut handoff_ids = self.handoff_ids.lock().unwrap().clone();
        handoff_ids.insert(message.id.clone());
        next.entry(session_id.to_string())
            .or_default()
            .push_back(message);
        let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let bytes = serde_json::to_vec(&MailboxFile {
            version: 1,
            items: next.clone(),
            handoff_ids: handoff_ids.clone(),
            processed_outbox_ids: self.processed_outbox_ids.lock().unwrap().clone(),
            human_message_receipts: self.human_message_receipts.lock().unwrap().clone(),
        })?;
        if let Err(error) =
            std::fs::write(&temporary, bytes).and_then(|_| std::fs::rename(&temporary, path))
        {
            let _ = std::fs::remove_file(&temporary);
            return Err(error.into());
        }
        *items = next;
        *self.handoff_ids.lock().unwrap() = handoff_ids;
        Ok(())
    }

    /// 补发提交回车：文本投递 SUBMIT_DELAY 后单独写 `\r`，实现免人工的自动提交。
    /// 写失败保留到下一轮 poll 重试；目标退出只清除本次文本阶段，原消息留在持久队列等恢复。
    /// 若人在这 600ms 内已手动回车，这次 `\r` 落在空输入框上是无害 no-op。
    pub fn flush_pending_submits(&self, session_state: &SessionManager, pty_state: &PtyManager) {
        self.flush_pending_submits_with_writer_at(session_state, Instant::now(), |pty_id, bytes| {
            pty_state.write(pty_id, bytes)
        })
    }

    fn flush_pending_submits_with_writer_at<F>(
        &self,
        session_state: &SessionManager,
        now: Instant,
        mut write: F,
    ) where
        F: FnMut(u32, &[u8]) -> Result<()>,
    {
        let _delivery = self.delivery_lock.lock().unwrap();
        let mut pending = self.pending_submits.lock().unwrap();
        pending.retain(|session_id, pending| {
            if !pending.submitted {
                let record = session_state.get(session_id);
                if record.as_ref().is_some_and(|record| crate::runtime_preferences::ensure_session_available(record).is_err()) { return true; }
                if now < pending.due { return true; }
                let pty_id = record.and_then(|record| record.pty_id);
                if pty_id != Some(pending.pty_id) {
                    self.update_delivery_stage(session_id, "queued", Some("目标进程已退出或更换，消息已保留等待恢复后重新发送".into()));
                    return false;
                }
                // A permission/input request can arrive after the paste but
                // before its delayed Enter. Never use a chat delivery to
                // acknowledge that request; keep the original message pending.
                if session_state.runtime_prompt_is_blocked(session_id)
                    || session_state.get(session_id).is_some_and(|record| {
                    record.status == crate::sessions::SessionStatus::Waiting
                }) {
                    pending.awaiting_prompt_after_blocker = true;
                    self.update_delivery_stage(session_id, "text_written", Some("目标正在等待人工处理，消息尚未提交；不会自动确认权限或输入提示".into()));
                    return true;
                }
                // Resolving a permission request can resume an active tool
                // turn. Wait for the input prompt, rather than injecting Enter
                // immediately after the Waiting status disappears.
                if pending.awaiting_prompt_after_blocker && !session_state.is_ready_for_prompt(session_id) {
                    return true;
                }
                if let Err(error) = write(pending.pty_id, b"\r") {
                    self.update_delivery_stage(session_id, "text_written", Some(format!("提交回车失败: {error:#}")));
                    return true;
                }
                pending.submitted = true;
            }
            // The terminal effect and disk write cannot be atomic. After Enter
            // succeeds, retries in this process only commit the durable ack.
            if self.acknowledge_submitted_message(session_id, &pending.message_id) {
                session_state.mark_prompt_dispatched(session_id);
                false
            } else { true }

        });
    }
}

fn outbox_message_id(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let name = path.file_name().context("outbox 消息缺少文件名")?;
    Ok(format!("outbox-{:x}", Sha256::digest(name.as_encoded_bytes())))
}

fn write_mailbox_file(path: &Path, file: &MailboxFile) -> Result<()> {
    use std::io::Write;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&temporary)?;
        output.write_all(&serde_json::to_vec(file)?)?;
        output.sync_all()?;
        drop(output);
        std::fs::rename(&temporary, path)?;
        Ok(())
    })();
    if result.is_err() { let _ = std::fs::remove_file(temporary); }
    result
}

fn agent_pair_key(a: &str, b: &str) -> (String, String) {
    if a <= b {
        (a.to_string(), b.to_string())
    } else {
        (b.to_string(), a.to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastTarget {
    pub session_id: String,
    pub title: String,
    pub provider_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BroadcastReceipt {
    pub target_count: usize,
    pub targets: Vec<BroadcastTarget>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionModeReceipt {
    pub target_mode: PermissionMode,
    pub target_count: usize,
    pub already_count: usize,
    pub sent_count: usize,
    pub awaiting_confirmation_count: usize,
    pub queued_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlWorkspaceTrustStatus {
    pub batch_count: usize,
    pub awaiting_count: usize,
    pub starting_count: usize,
    pub missing_count: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ControlWorkspaceTrustReceipt {
    pub confirmed_count: usize,
    pub skipped_count: usize,
    pub failed_session_ids: Vec<String>,
}

fn contains_provider_alias(text: &str, alias: &str) -> bool {
    if alias.is_empty() {
        return false;
    }
    if !alias.is_ascii() {
        return text.contains(alias);
    }
    text.match_indices(alias).any(|(index, value)| {
        let before = text[..index].chars().next_back();
        let after = text[index + value.len()..].chars().next();
        before.is_none_or(|ch| !ch.is_ascii_alphanumeric())
            && after.is_none_or(|ch| !ch.is_ascii_alphanumeric())
    })
}

fn parse_explicit_launch(message: &str, providers: &[ProviderConfig]) -> Option<ControlPlan> {
    let message = message.trim().to_lowercase();
    if !["打开", "启动", "新建", "创建"]
        .iter()
        .any(|verb| message.contains(verb))
        || ["yolo", "auto"].iter().any(|mode| message.contains(mode))
    {
        return None;
    }

    let mut launches = Vec::<ControlLaunch>::new();
    for segment in message
        .split([',', '，', '、', ';', '；', '\n'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
    {
        let matches = providers
            .iter()
            .filter(|provider| {
                contains_provider_alias(segment, &provider.id.to_lowercase())
                    || contains_provider_alias(segment, &provider.name.to_lowercase())
            })
            .collect::<Vec<_>>();
        if matches.len() != 1 {
            return None;
        }
        let count = segment
            .split(|ch: char| !ch.is_ascii_digit())
            .find(|part| !part.is_empty())
            .and_then(|part| part.parse::<u32>().ok())
            .unwrap_or(1);
        if count == 0 {
            return None;
        }
        let provider = matches[0];
        if let Some(existing) = launches
            .iter_mut()
            .find(|launch| launch.provider_id == provider.id)
        {
            existing.count = existing.count.saturating_add(count);
        } else {
            launches.push(ControlLaunch {
                provider_id: provider.id.clone(),
                count,
                mode: ControlLaunchMode::Default,
            });
        }
    }
    if launches.is_empty() {
        return None;
    }
    let reply = launches
        .iter()
        .map(|launch| format!("{} ×{}", launch.provider_id, launch.count))
        .collect::<Vec<_>>()
        .join("、");
    Some(ControlPlan {
        intent: ControlIntent::Launch,
        reply: format!("准备打开 {reply}"),
        launches,
        prompt: None,
    })
}

/// 总控调用要忽略从本进程继承的 provider 路由（env 变量全部清掉），
/// 但保留用户自己的 `~/.claude/settings.json`（`--setting-sources user`）——
/// 本地 claude CLI 常以第三方 provider 的 env 块配置在 user settings 里，
/// 完全禁用会让总控退化成官方登录态，未登录用户将无法使用总控。
fn isolate_local_claude(command: &mut Command) {
    for variable in CLEAR_VARS {
        command.env_remove(variable);
    }
    command.arg("--setting-sources").arg("user");
}

pub fn interpret_with_claude(
    message: &str,
    history: &[ControlHistoryMessage],
    providers: &[ProviderConfig],
) -> Result<ControlPlan> {
    let message = message.trim();
    if message.is_empty() {
        return Err(anyhow!("总控消息不能为空"));
    }

    let allowed_ids = providers
        .iter()
        .map(|provider| provider.id.clone())
        .collect::<Vec<_>>();
    if let Some(plan) = parse_explicit_launch(message, providers) {
        crate::multi_agent::AgentRuntimeKind::ClaudeCode.ensure_available()?;
        return Ok(normalize_plan(plan, &allowed_ids));
    }
    let _coordinator_lease =
        crate::runtime_preferences::acquire(crate::multi_agent::AgentRuntimeKind::ClaudeCode)?;
    let provider_context = providers
        .iter()
        .map(|provider| {
            let model = provider
                .env
                .get("ANTHROPIC_MODEL")
                .or_else(|| provider.env.get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
                .map(String::as_str)
                .unwrap_or("未指定模型");
            format!("- id={}；名称={}；模型={model}", provider.id, provider.name)
        })
        .collect::<Vec<_>>()
        .join("\n");
    let system_prompt = format!(
        r#"你是 CrewVela 的本地总控指令解析器。你的唯一职责是把用户的中文自然语言转换为结构化执行计划，不能调用工具，也不能自行执行任务。

可用 Provider：
{provider_context}

规则：
1. 用户要求新建、打开或启动会话时，intent=launch，并把每个 Provider 的精确 id、数量和启动模式放进 launches。
2. 用户要求把一段 Prompt 发送给全部、所有或当前会话时，intent=broadcast；prompt 必须逐字保留用户要发送的任务内容，不能润色。
3. 用户明确要求关闭、停止或结束所有会话时，intent=stop_all，launches 必须为空且 prompt=null。只要求关闭单个或部分会话时不支持，intent=unknown。
4. 用户明确要求删除所有或全部会话记录时，intent=delete_all，launches 必须为空且 prompt=null。不要把“清空总控记录”“关闭所有会话”或删除单个/部分会话解释为 delete_all。
5. 用户询问能力或用法时 intent=help；无法确定时 intent=unknown，绝不猜测 Provider。
6. “几个会话”的总数必须等于 launches 中 count 之和。最多创建 {MAX_CONTROL_SESSIONS} 个。
7. Provider 名称、模型名或常见简称应映射到上面最接近且唯一的 Provider id；不能返回列表外的 id。
8. 每个 launch 的 mode 必须是 default、auto 或 yolo。用户没有指定时使用 default；Auto 对应 auto，YOLO 对应 yolo。模式修饰单个 Provider 时只应用到该 launch；修饰整条指令时应用到全部 launch。
9. Auto 与 YOLO 不能同时应用到同一个 launch；语义冲突时该 launch 使用 default，并在 reply 中说明冲突。
10. reply 使用简短中文说明即将执行的操作和模式，不使用 Markdown。
"#
    );
    let recent_history = history
        .iter()
        .rev()
        .take(8)
        .cloned()
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>();
    let user_prompt = format!(
        "最近对话：\n{}\n\n当前用户消息：\n{}",
        serde_json::to_string(&recent_history)?,
        message
    );
    let schema = serde_json::json!({
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "intent": {
                "type": "string",
                "enum": ["launch", "broadcast", "stop_all", "delete_all", "help", "unknown"]
            },
            "reply": {"type": "string"},
            "launches": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "properties": {
                        "providerId": {"type": "string"},
                        "count": {"type": "integer", "minimum": 1, "maximum": MAX_CONTROL_SESSIONS},
                        "mode": {
                            "type": "string",
                            "enum": ["default", "auto", "yolo"]
                        }
                    },
                    "required": ["providerId", "count", "mode"]
                }
            },
            "prompt": {"type": ["string", "null"]}
        },
        "required": ["intent", "reply", "launches", "prompt"]
    })
    .to_string();

    let claude = resolve_claude()?;
    let mut command = crate::title_summary::claude_command(&claude)?;
    isolate_local_claude(&mut command);
    command
        .arg("-p")
        .arg("--output-format")
        .arg("json")
        .arg("--json-schema")
        .arg(schema)
        .arg("--no-session-persistence")
        .arg("--safe-mode")
        .arg("--tools")
        .arg("")
        .arg("--permission-mode")
        .arg("dontAsk")
        .arg("--system-prompt")
        .arg(system_prompt)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(home) = dirs::home_dir() {
        command.current_dir(home);
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }

    let output = crate::title_summary::run_cli_with_timeout(
        command, Some(user_prompt.into_bytes()), CONTROL_TIMEOUT, 8 * 1024 * 1024, "本地 Claude Code 总控解析",
    )?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = describe_claude_failure(&stdout, &stderr);
        return Err(anyhow!(
            "本地 Claude Code 总控解析失败{}",
            if detail.is_empty() {
                String::new()
            } else {
                format!("：{detail}")
            }
        ));
    }
    let raw = String::from_utf8(output.stdout).context("Claude Code 返回了无效 UTF-8")?;
    let plan = parse_claude_output(&raw)?;
    Ok(normalize_plan(plan, &allowed_ids))
}

fn parse_claude_output(raw: &str) -> Result<ControlPlan> {
    let root: serde_json::Value =
        serde_json::from_str(raw.trim()).context("解析 Claude Code JSON 输出失败")?;
    // CLI 偶尔 exit 0 但 envelope 标了 is_error，先于结构化解析拦截。
    if root.get("is_error").and_then(serde_json::Value::as_bool) == Some(true) {
        let detail = summarize_claude_output(raw).unwrap_or_default();
        let detail = if detail.is_empty() {
            "未知错误".to_string()
        } else {
            detail
        };
        return Err(anyhow!("Claude Code 返回错误：{detail}"));
    }
    if let Some(structured) = root.get("structured_output") {
        return serde_json::from_value(structured.clone())
            .context("解析 Claude Code 结构化计划失败");
    }
    if let Some(result) = root.get("result").and_then(|value| value.as_str()) {
        return serde_json::from_str(result).context("解析 Claude Code 文本计划失败");
    }
    serde_json::from_value(root).context("Claude Code 未返回可识别的总控计划")
}

/// claude CLI 非零退出时把输出整理成可读错误文案：stderr 优先（真正的 CLI 报错）；
/// stdout 是 result envelope（is_error/result/usage 统计）时只提取可诊断信息，
/// 不把整坨 usage JSON 甩给用户。
fn describe_claude_failure(stdout: &str, stderr: &str) -> String {
    for detail in [stderr, stdout] {
        if let Some(summary) = summarize_claude_output(detail) {
            return summary;
        }
    }
    String::new()
}

fn summarize_claude_output(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Some(raw.chars().take(400).collect());
    };
    let is_envelope = value.get("type").and_then(serde_json::Value::as_str) == Some("result")
        || value.get("result").is_some()
        || value.get("subtype").is_some()
        || (value.get("usage").is_some() && value.get("session_id").is_some());
    if !is_envelope {
        return Some(raw.chars().take(400).collect());
    }
    let api_error_status = value
        .get("api_error_status")
        .and_then(serde_json::Value::as_i64);
    let result = value
        .get("result")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|result| !result.is_empty());
    // CLI 本地未登录时短路，result 固定是这个文案，api_error_status 可能为 null。
    if result.is_some_and(|result| result.contains("Not logged in")) {
        return Some(
            "本地 Claude Code 未登录或无可用 Provider 配置：请运行 claude 并执行 /login，或检查 ~/.claude/settings.json 的 env 配置"
                .to_string(),
        );
    }
    if let Some(result) = result {
        let text: String = result.chars().take(400).collect();
        return Some(match api_error_status {
            Some(status) => format!("{text}（接口返回 {status}）"),
            None => text,
        });
    }
    if let Some(status) = api_error_status {
        return Some(format!(
            "Claude Code 接口调用失败（HTTP {status}），请检查登录态、额度或网络"
        ));
    }
    if usage_all_zero(&value) {
        return Some(
            "Claude Code 没有返回任何内容（input_tokens=0），可能是本地登录态不可用、额度耗尽或网络异常"
                .to_string(),
        );
    }
    value
        .get("subtype")
        .and_then(serde_json::Value::as_str)
        .filter(|subtype| !subtype.is_empty())
        .map(|subtype| format!("Claude Code 返回错误结果（{subtype}）"))
}

/// usage 里 input/output token 全为 0，说明这次 API 请求根本没有成功发生。
fn usage_all_zero(envelope: &serde_json::Value) -> bool {
    let Some(usage) = envelope.get("usage") else {
        return false;
    };
    let tokens = |key: &str| {
        usage
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    tokens("input_tokens") == 0 && tokens("output_tokens") == 0
}

fn normalize_plan(mut plan: ControlPlan, allowed_provider_ids: &[String]) -> ControlPlan {
    let allowed = allowed_provider_ids.iter().collect::<HashSet<_>>();
    plan.reply = plan.reply.trim().to_string();
    plan.prompt = plan
        .prompt
        .take()
        .map(|prompt| prompt.trim().to_string())
        .filter(|prompt| !prompt.is_empty());

    if plan.intent == ControlIntent::Launch {
        let unavailable = plan
            .launches
            .iter()
            .filter(|launch| !allowed.contains(&launch.provider_id))
            .map(|launch| launch.provider_id.clone())
            .collect::<Vec<_>>();
        if !unavailable.is_empty() {
            plan.intent = ControlIntent::Unknown;
            plan.reply = format!(
                "未找到 Provider：{}。请先在设置中新增或完成配置。",
                unavailable.join("、")
            );
            plan.launches.clear();
            return plan;
        }

        let mut remaining = MAX_CONTROL_SESSIONS;
        let mut launches = Vec::new();
        for mut launch in plan.launches {
            if !allowed.contains(&launch.provider_id) || launch.count == 0 || remaining == 0 {
                continue;
            }
            launch.count = launch.count.min(remaining);
            remaining -= launch.count;
            launches.push(launch);
        }
        plan.launches = launches;
        if plan.launches.is_empty() {
            plan.intent = ControlIntent::Unknown;
            plan.reply = "没有找到可执行的 Provider，请先在设置中完成配置。".into();
        }
    } else {
        plan.launches.clear();
    }

    if plan.intent == ControlIntent::Broadcast && plan.prompt.is_none() {
        plan.intent = ControlIntent::Unknown;
        plan.reply = "没有识别到需要广播的 Prompt，请明确写出要发送的内容。".into();
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::{
        ApprovalPolicy, NewRuntimeSession, NewSession, SessionKind, SessionStatus, TerminalShell,
    };

    fn provider(id: &str, name: &str) -> ProviderConfig {
        ProviderConfig {
            id: id.into(),
            name: name.into(),
            base_url: "https://example.com".into(),
            auth_token_ref: id.into(),
            env: Default::default(),
            timeout_ms: None,
            disable_nonessential_traffic: false,
            builtin: true,
            hidden: false,
            meta: crate::providers::ProviderMeta::default(),
        }
    }

    #[test]
    fn explicit_provider_launch_is_parsed_without_claude() {
        let providers = [
            provider("deepseek", "DeepSeek"),
            provider("glm", "智谱 GLM"),
            provider("kimi", "Kimi For Coding"),
            provider("minimax", "MiniMax"),
        ];

        let plan = interpret_with_claude(
            "打开1个deepseek、1个glm、1个kimi、1个minimax",
            &[],
            &providers,
        )
        .expect("明确的 Provider 启动命令不应依赖 Claude");

        assert_eq!(plan.intent, ControlIntent::Launch);
        assert_eq!(plan.launches.len(), 4);
        assert!(plan.launches.iter().all(|launch| launch.count == 1));
        assert_eq!(plan.launches[0].provider_id, "deepseek");
        assert_eq!(plan.launches[3].provider_id, "minimax");
        assert!(parse_explicit_launch("打开1个unknown、1个glm", &providers).is_none());
    }

    #[test]
    fn local_controller_ignores_inherited_env_but_keeps_user_settings() {
        let mut command = Command::new("claude");
        command.env("ANTHROPIC_MODEL", "k3");

        isolate_local_claude(&mut command);

        assert!(command
            .get_envs()
            .any(|(name, value)| { name == "ANTHROPIC_MODEL" && value.is_none() }));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            ["--setting-sources", "user"]
        );
    }

    #[test]
    fn standalone_kimi_message_waits_until_the_runtime_prompt_is_ready() {
        let root =
            std::env::temp_dir().join(format!("crewvela-kimi-ready-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let created = manager
            .create_runtime(NewRuntimeSession {
                runtime_kind: "kimi_code".into(),
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 9).unwrap();
        let queue = SessionActionQueue::new();
        queue.enqueue_message(&created.id, QueuedMessage::new("你好 Kimi".into(), None));
        let writes = std::cell::RefCell::new(Vec::<(u32, Vec<u8>)>::new());
        let mut writer = |pty_id: u32, bytes: &[u8]| {
            writes.borrow_mut().push((pty_id, bytes.to_vec()));
            Ok(())
        };

        assert!(queue
            .flush_ready_with_writer_at(&manager, Instant::now(), &mut writer)
            .is_empty());
        assert!(manager.mark_runtime_prompt_ready(&created.id));
        assert_eq!(
            queue.flush_ready_with_writer_at(&manager, Instant::now(), &mut writer),
            vec![created.id.clone()],
        );
        assert_eq!(
            writes.borrow().as_slice(),
            &[(9, "\x1b[200~你好 Kimi\x1b[201~".as_bytes().to_vec())],
        );
        assert!(!manager.is_ready_for_prompt(&created.id));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn concurrent_message_commit_does_not_remove_a_successor() {
        let queue = SessionActionQueue::new();
        let first = QueuedMessage::new("first".into(), None);
        let second = QueuedMessage::new("second".into(), None);
        queue.enqueue_message("target", first.clone());
        queue.enqueue_message("target", second.clone());
        assert!(queue.commit_if_id_matches("target", &first.id));
        assert!(!queue.commit_if_id_matches("target", &first.id));
        assert_eq!(queue.peek_next("target").unwrap().id, second.id);
    }

    #[test]
    fn concurrent_mailbox_mutations_survive_reopen_without_losing_tail_messages() {
        let root = std::env::temp_dir().join(format!(
            "crewvela-concurrent-mailbox-{}",
            uuid::Uuid::new_v4()
        ));
        let queue = std::sync::Arc::new(
            SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap(),
        );
        let first = QueuedMessage::new("first".into(), None);
        queue.enqueue_message("target", first.clone());
        let mut threads = Vec::new();
        for writer in 0..4 {
            let queue = queue.clone();
            threads.push(std::thread::spawn(move || {
                let mut ids = Vec::new();
                for index in 0..20 {
                    let message = QueuedMessage::new(format!("{writer}:{index}"), None);
                    ids.push(message.id.clone());
                    queue.enqueue_message("target", message);
                }
                ids
            }));
        }
        let committer = queue.clone();
        let id = first.id;
        let commit = std::thread::spawn(move || {
            for _ in 0..80 {
                committer.commit_if_id_matches("target", &id);
            }
        });
        let expected = threads
            .into_iter()
            .flat_map(|thread| thread.join().unwrap())
            .collect::<HashSet<_>>();
        commit.join().unwrap();
        let reopened =
            SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        let actual = reopened.items.lock().unwrap()["target"]
            .iter()
            .map(|message| message.id.clone())
            .collect::<HashSet<_>>();
        assert_eq!(actual, expected);
        assert_eq!(reopened.items.lock().unwrap()["target"].len(), 80);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn retained_runtime_cli_outbox_delivery_matrix_preserves_sender_and_multiline() {
        let root =
            std::env::temp_dir().join(format!("crewvela-message-matrix-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let queue =
            SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        let runtimes = [
            "claude_code",
            "codex",
            "kimi_code",
            "pi",
            "opencode",
        ];
        let targets = runtimes
            .into_iter()
            .enumerate()
            .map(|(index, kind)| {
                let session = manager
                    .create_runtime(NewRuntimeSession {
                        runtime_kind: kind.into(),
                        cwd: ".".into(),
                        project_id: None,
                    })
                    .unwrap();
                manager
                    .attach_runtime(&session.id, index as u32 + 1)
                    .unwrap();
                if kind == "pi" {
                    manager
                        .configure_pi_readiness(&mut std::process::Command::new("pi"), &session.id)
                        .unwrap();
                }
                session
            })
            .collect::<Vec<_>>();
        for (sender_index, sender) in targets.iter().enumerate() {
            for (target_index, target) in targets.iter().enumerate() {
                let message = OutboxMessage {
                    to: target.id.clone(),
                    from: Some(MessageOrigin {
                        session_id: sender.id.clone(),
                        label: sender.title.clone(),
                    }),
                    body: format!("{} -> {}\nsecond line", sender_index, target_index),
                };
                crate::messaging::write_outbox(&root.join("outbox"), &message).unwrap();
                assert_eq!(queue.ingest_outbox(&manager, |_| Ok(())).unwrap(), 1);
                let pending = queue.peek_next(&target.id).unwrap();
                assert_eq!(pending.from.as_ref().unwrap().session_id, sender.id);
                manager.mark_runtime_prompt_ready(&target.id);
                let mut writes = Vec::new();
                let now = Instant::now();
                if matches!(target.agent_runtime_kind.as_deref(), Some("pi")) {
                    assert!(queue
                        .flush_ready_with_writer_at(&manager, now, |_, _| panic!(
                            "Pi API must bypass TUI"
                        ))
                        .is_empty());
                    let inbox = root
                        .join(format!(
                            "{}-inbox",
                            target.agent_runtime_kind.as_deref().unwrap()
                        ))
                        .join(&target.id);
                    let request: serde_json::Value = serde_json::from_slice(
                        &std::fs::read(inbox.join(format!("{}.request.json", pending.id))).unwrap(),
                    )
                    .unwrap();
                    let body = request["body"].as_str().unwrap();
                    assert!(body.contains(&sender.id));
                    assert!(body.contains("\nsecond line"));
                    std::fs::write(
                        inbox.join(format!("{}.accepted.json", pending.id)),
                        serde_json::to_vec(&serde_json::json!({
                            "sessionId": target.id, "messageId": pending.id, "status": "accepted",
                        }))
                        .unwrap(),
                    )
                    .unwrap();
                    assert_eq!(
                        queue.flush_ready_with_writer_at(&manager, now, |_, _| panic!(
                            "Pi API must bypass TUI"
                        )),
                        vec![target.id.clone()]
                    );
                    assert!(queue.peek_next(&target.id).is_none());
                    continue;
                }
                assert_eq!(
                    queue.flush_ready_with_writer_at(&manager, now, |pty, bytes| {
                        writes.push((pty, bytes.to_vec()));
                        Ok(())
                    }),
                    vec![target.id.clone()]
                );
                let pasted = String::from_utf8(writes[0].1.clone()).unwrap();
                assert!(pasted.starts_with("\x1b[200~"));
                assert!(pasted.ends_with("\x1b[201~"));
                assert!(pasted.contains(&sender.id));
                assert!(pasted.contains("\nsecond line"));
                queue.flush_pending_submits_with_writer_at(
                    &manager,
                    now + SUBMIT_DELAY,
                    |_, bytes| {
                        assert_eq!(bytes, b"\r");
                        Ok(())
                    },
                );
                assert!(!manager.is_ready_for_prompt(&target.id));
                assert!(queue.peek_next(&target.id).is_none());
            }
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn pi_inbox_keeps_existing_session_and_only_commits_confirmed_receipt() {
        let root = std::env::temp_dir().join(format!("cc-pi-inbox-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let session = manager
            .create_runtime(crate::sessions::NewRuntimeSession {
                runtime_kind: "pi".into(),
                cwd: ".".into(),
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&session.id, 42).unwrap();
        let mut command = std::process::Command::new("pi");
        manager
            .configure_pi_readiness(&mut command, &session.id)
            .unwrap();
        let queue = SessionActionQueue::new();
        queue.enqueue_message(&session.id, QueuedMessage::new("first".into(), None));
        queue.enqueue_message(&session.id, QueuedMessage::new("second".into(), None));
        let directory = root.join("pi-inbox").join(&session.id);
        let now = Instant::now();
        for expected in ["first", "second"] {
            let message = queue.peek_next(&session.id).unwrap();
            // No startup event is required to queue safely; Pi's programmatic receiver owns busy state.
            assert!(queue
                .flush_ready_with_writer_at(&manager, now, |_, _| panic!(
                    "Pi must never receive keyboard paste"
                ))
                .is_empty());
            let request = directory.join(format!("{}.request.json", message.id));
            let payload: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&request).unwrap()).unwrap();
            assert_eq!(payload["body"], expected);
            assert!(!payload["token"].as_str().unwrap().is_empty());
            assert_eq!(queue.peek_next(&session.id).unwrap().id, message.id);
            std::fs::rename(
                &request,
                directory.join(format!("{}.processing.json", message.id)),
            )
            .unwrap();
            // Claimed but unconfirmed receipt must not be resent, including after startup token rotates.
            manager
                .configure_pi_readiness(&mut command, &session.id)
                .unwrap();
            assert!(queue
                .flush_ready_with_writer_at(&manager, now, |_, _| panic!("no keyboard input"))
                .is_empty());
            assert!(!request.exists());
            std::fs::write(
                directory.join(format!("{}.accepted.json", message.id)),
                serde_json::to_vec(&serde_json::json!({
                    "sessionId": session.id, "messageId": message.id, "status": "accepted",
                }))
                .unwrap(),
            )
            .unwrap();
            assert_eq!(
                queue.flush_ready_with_writer_at(&manager, now, |_, _| panic!("no keyboard input")),
                vec![session.id.clone()]
            );
        }
        queue.flush_pending_submits_with_writer_at(&manager, now + SUBMIT_DELAY, |_, _| {
            panic!("Pi must not receive delayed Enter")
        });
        assert!(queue.peek_next(&session.id).is_none());
        assert_eq!(manager.list().len(), 1);
        assert!(manager
            .get(&session.id)
            .unwrap()
            .multi_agent_agent_id
            .is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    fn session(id: &str, pty_id: Option<u32>) -> SessionRecord {
        SessionRecord {
            launch_model: None, model_assignment: None,            id: id.into(),
            claude_session_id: id.into(),
            kind: SessionKind::Claude,
            shell: None,
            multi_agent_project_id: None,
            multi_agent_agent_id: None,
            agent_runtime_kind: None,
            retired_agent_runtime: false,
            agent_avatar_id: None,
            runtime_command_host: false,
            project_id: None,
            worktree: None,
            provider_id: "glm".into(),
            provider_name: "Z.AI GLM".into(),
            cwd: ".".into(),
            title: format!("会话 {id}"),
            status: if pty_id.is_some() {
                SessionStatus::Completed
            } else {
                SessionStatus::Stopped
            },
            pty_id,
            yolo: false,
            auto_mode: false,
            permission_mode: Some(crate::sessions::PermissionMode::Default),
            permission_mode_revision: 1,
            approval_policy: ApprovalPolicy::default(),
            waiting_reason: None,
            waiting_since: None,
            title_locked: false,
            exit_code: None,
            last_error: None,
            archived_at: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn terminal_session(id: &str, pty_id: Option<u32>) -> SessionRecord {
        let mut record = session(id, pty_id);
        record.kind = SessionKind::Terminal;
        record.shell = Some(TerminalShell::Powershell);
        record.provider_id = "local-terminal".into();
        record.provider_name = "PowerShell".into();
        record.permission_mode = None;
        record
    }

    #[test]
    fn claude_structured_output_preserves_provider_counts() {
        let raw = r#"{
            "type": "result",
            "structured_output": {
                "intent": "launch",
                "reply": "准备创建 6 个会话",
                "launches": [
                    {"providerId": "deepseek", "count": 1},
                    {"providerId": "glm", "count": 1},
                    {"providerId": "kimi", "count": 1},
                    {"providerId": "minimax", "count": 1},
                    {"providerId": "agnes", "count": 2}
                ],
                "prompt": null
            }
        }"#;

        let plan = parse_claude_output(raw).expect("应解析 Claude 结构化结果");

        assert_eq!(plan.intent, ControlIntent::Launch);
        assert_eq!(plan.launches.len(), 5);
        assert_eq!(plan.launches.iter().map(|item| item.count).sum::<u32>(), 6);
    }

    #[test]
    fn claude_structured_output_supports_stopping_all_sessions() {
        let raw = r#"{
            "type": "result",
            "structured_output": {
                "intent": "stop_all",
                "reply": "准备关闭所有会话",
                "launches": [],
                "prompt": null
            }
        }"#;

        let plan = parse_claude_output(raw).expect("应解析关闭全部会话计划");

        assert_eq!(plan.intent, ControlIntent::StopAll);
        assert!(plan.launches.is_empty());
        assert!(plan.prompt.is_none());
    }

    #[test]
    fn claude_structured_output_supports_deleting_all_sessions() {
        let raw = r#"{
            "type": "result",
            "structured_output": {
                "intent": "delete_all",
                "reply": "准备删除所有会话",
                "launches": [],
                "prompt": null
            }
        }"#;

        let plan = parse_claude_output(raw).expect("应解析删除全部会话计划");

        assert_eq!(plan.intent, ControlIntent::DeleteAll);
        assert!(plan.launches.is_empty());
        assert!(plan.prompt.is_none());
    }

    #[test]
    fn claude_structured_output_preserves_launch_modes() {
        let raw = r#"{
            "type": "result",
            "structured_output": {
                "intent": "launch",
                "reply": "准备按指定模式创建会话",
                "launches": [
                    {"providerId": "deepseek", "count": 1, "mode": "auto"},
                    {"providerId": "glm", "count": 1, "mode": "yolo"},
                    {"providerId": "kimi", "count": 1, "mode": "default"}
                ],
                "prompt": null
            }
        }"#;

        let plan = parse_claude_output(raw).expect("应解析带启动模式的结构化结果");

        assert_eq!(plan.launches[0].mode, ControlLaunchMode::Auto);
        assert_eq!(plan.launches[1].mode, ControlLaunchMode::Yolo);
        assert_eq!(plan.launches[2].mode, ControlLaunchMode::Default);
    }

    #[test]
    fn claude_failure_extracts_result_text_instead_of_usage_json() {
        let stdout = r#"{"type":"result","is_error":true,"api_error_status":500,"duration_api_ms":0,"stop_reason":"stop_sequence","session_id":"3ecf0b27-d814-48b8-a6f4-86dd65678aa7","total_cost_usd":0,"usage":{"output_tokens_details":{"thinking_tokens":0},"input_tokens":0,"output_tokens":0},"result":"API Error: 500 应用额度不可用"}"#;

        let detail = describe_claude_failure(stdout, "");

        assert!(detail.contains("API Error: 500"));
        assert!(detail.contains("接口返回 500"));
        assert!(!detail.contains("duration_api_ms"));
        assert!(!detail.contains("total_cost_usd"));
    }

    #[test]
    fn claude_failure_without_result_hints_at_quota_exhaustion() {
        let stdout = r#"{"duration_api_ms":0,"stop_reason":"stop_sequence","session_id":"3ecf0b27-d814-48b8-a6f4-86dd65678aa7","total_cost_usd":0,"usage":{"output_tokens_details":{"thinking_tokens":0},"input_tokens":0,"output_tokens":0}}"#;

        let detail = describe_claude_failure(stdout, "");

        assert!(detail.contains("额度"));
        assert!(!detail.contains("total_cost_usd"));
        assert!(!detail.contains("usage"));
    }

    #[test]
    fn claude_failure_reports_api_error_status_without_result() {
        let stdout = r#"{"is_error":true,"api_error_status":429,"session_id":"3ecf0b27","usage":{"input_tokens":0,"output_tokens":0}}"#;

        let detail = describe_claude_failure(stdout, "");

        assert!(detail.contains("HTTP 429"));
        assert!(!detail.contains("session_id"));
    }

    #[test]
    fn claude_failure_translates_not_logged_in_hint() {
        // CLI 本地未登录短路：result 固定文案，api_error_status 可能为 null（401 时为 401）。
        let stdout = r#"{"is_error":true,"api_error_status":null,"terminal_reason":"api_error","session_id":"b86d1c5d","usage":{"input_tokens":0,"output_tokens":0},"result":"Not logged in · Please run /login","type":"result"}"#;

        let detail = describe_claude_failure(stdout, "");

        assert!(detail.contains("未登录"));
        assert!(detail.contains("/login"));
        assert!(!detail.contains("Not logged in"));
    }

    #[test]
    fn claude_output_flags_error_envelope_even_on_success_exit() {
        // CLI 偶尔 exit 0 但 envelope 标了 is_error，应在结构化解析前拦截。
        let raw = r#"{"is_error":true,"api_error_status":401,"session_id":"46b3","usage":{"input_tokens":0,"output_tokens":0},"result":"Not logged in · Please run /login","type":"result"}"#;

        let error = parse_claude_output(raw).expect_err("错误 envelope 应被拦截");

        assert!(error.to_string().contains("未登录"));
    }

    #[test]
    fn claude_failure_prefers_stderr_and_truncates_plain_text() {
        let detail = describe_claude_failure("{\"unused\":1}", "claude: Invalid --safe-mode flag");
        assert_eq!(detail, "claude: Invalid --safe-mode flag");

        let long = "x".repeat(500);
        assert_eq!(
            summarize_claude_output(&long)
                .expect("非 JSON 输出应原样截断")
                .chars()
                .count(),
            400
        );
    }

    #[test]
    fn legacy_control_launch_without_mode_defaults_to_default() {
        let launch: ControlLaunch =
            serde_json::from_str(r#"{"providerId":"glm","count":1}"#).unwrap();

        assert_eq!(launch.mode, ControlLaunchMode::Default);
    }

    #[test]
    fn launch_plan_caps_total_count() {
        let plan = ControlPlan {
            intent: ControlIntent::Launch,
            reply: "执行批量创建".into(),
            launches: vec![
                ControlLaunch {
                    provider_id: "glm".into(),
                    count: 10,
                    mode: ControlLaunchMode::Default,
                },
                ControlLaunch {
                    provider_id: "kimi".into(),
                    count: 10,
                    mode: ControlLaunchMode::Default,
                },
            ],
            prompt: None,
        };

        let normalized = normalize_plan(plan, &["glm".into(), "kimi".into()]);

        assert_eq!(normalized.launches.len(), 2);
        assert_eq!(
            normalized
                .launches
                .iter()
                .map(|item| item.count)
                .sum::<u32>(),
            MAX_CONTROL_SESSIONS
        );
    }

    #[test]
    fn launch_plan_rejects_unknown_provider_instead_of_partially_launching() {
        let plan = ControlPlan {
            intent: ControlIntent::Launch,
            reply: "准备创建 2 个会话".into(),
            launches: vec![
                ControlLaunch {
                    provider_id: "glm".into(),
                    count: 1,
                    mode: ControlLaunchMode::Default,
                },
                ControlLaunch {
                    provider_id: "agnes".into(),
                    count: 1,
                    mode: ControlLaunchMode::Default,
                },
            ],
            prompt: None,
        };

        let normalized = normalize_plan(plan, &["glm".into()]);

        assert_eq!(normalized.intent, ControlIntent::Unknown);
        assert!(normalized.launches.is_empty());
        assert!(normalized.reply.contains("agnes"));
    }

    #[test]
    fn mailbox_survives_queue_reopen() {
        let root = std::env::temp_dir().join(format!("crewvela-mailbox-{}", uuid::Uuid::new_v4()));
        let mailbox = root.join("mailbox.json");
        let outbox = root.join("outbox");
        let origin = MessageOrigin {
            session_id: "sender-1".into(),
            label: "需求拆解 (Z.AI GLM)".into(),
        };

        let queue = SessionActionQueue::open_at(mailbox.clone(), outbox.clone()).unwrap();
        queue.enqueue_message(
            "session-b",
            QueuedMessage::new("帮我看下接口定义".into(), Some(origin.clone())),
        );
        drop(queue);

        let reopened = SessionActionQueue::open_at(mailbox, outbox).unwrap();
        let pending = reopened.peek_next("session-b").unwrap();
        assert_eq!(pending.body, "帮我看下接口定义");
        assert_eq!(pending.from, Some(origin));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn agent_message_delivery_wraps_origin_and_reply_hint() {
        let message = QueuedMessage::new(
            "接口改完了，帮忙回归".into(),
            Some(MessageOrigin {
                session_id: "sender-9".into(),
                label: "接口检查 (Kimi-K2)".into(),
            }),
        );

        let delivered = render_delivery(&message);

        assert!(delivered.contains("来自 接口检查 (Kimi-K2)"));
        assert!(delivered.contains("接口改完了，帮忙回归"));
        assert!(delivered.contains("send --to 'sender-9'"));
        assert!(delivered.contains("--body '内容'"));
    }

    #[test]
    fn reply_commands_quote_executable_and_original_sender_for_each_shell() {
        let commands = render_reply_commands("C:\\User's folder\\crewvela.exe", "sender'9");
        assert!(commands
            .contains("PowerShell：& 'C:\\User''s folder\\crewvela.exe' send --to 'sender''9'"));
        let bash_expected = if cfg!(windows) {
            "Bash：'C:/User'\"'\"'s folder/crewvela.exe' send --to 'sender'\"'\"'9'"
        } else {
            "Bash：'C:\\User'\"'\"'s folder\\crewvela.exe' send --to 'sender'\"'\"'9'"
        };
        assert!(commands.contains(bash_expected));
    }

    #[test]
    fn plain_message_delivery_is_verbatim() {
        let message = QueuedMessage::new("  逐字\n保留  ".into(), None);

        assert_eq!(render_delivery(&message), "  逐字\n保留  ");
    }

    #[test]
    fn runtime_switch_rejected_sender_does_not_clear_healthy_target_messages() {
        let root =
            std::env::temp_dir().join(format!("crewvela-retired-outbox-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let sender = manager
            .create_agent(crate::sessions::NewAgentSession {
                avatar_id: None,
                project_id: "project".into(),
                agent_id: "agent".into(),
                runtime_kind: "pi".into(),
                agent_name: "Pi".into(),
                cwd: ".".into(),
            })
            .unwrap();
        let target = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        let queue =
            SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        queue.enqueue_message(&target.id, QueuedMessage::new("合法待发".into(), None));
        manager
            .retire_agent_runtime_sessions("project", "agent")
            .unwrap();
        let message = OutboxMessage {
            to: target.id.clone(),
            from: Some(MessageOrigin {
                session_id: sender.id.clone(),
                label: "旧Pi".into(),
            }),
            body: "旧会话迟到消息".into(),
        };
        std::fs::write(
            root.join("outbox/late.json"),
            serde_json::to_vec(&message).unwrap(),
        )
        .unwrap();
        assert_eq!(
            queue
                .ingest_outbox(&manager, |_| panic!("退役来源不应进入投递"))
                .unwrap(),
            0
        );
        assert_eq!(queue.peek_next(&target.id).unwrap().body, "合法待发");
        let records = queue.delivery_records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].stage, "failed");
        assert!(records[0].error.is_some());
        queue.fail_retired_session(&target.id);
        assert!(queue.peek_next(&target.id).is_none());
        assert_eq!(queue.delivery_records().len(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ingest_outbox_enqueues_messages_and_drops_unknown_targets() {
        let root = std::env::temp_dir().join(format!("crewvela-outbox-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        let queue =
            SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        let outbox_dir = root.join("outbox");
        let valid = OutboxMessage {
            to: created.id.clone(),
            from: Some(MessageOrigin {
                session_id: "sender-1".into(),
                label: "发送方".into(),
            }),
            body: "请回归接口".into(),
        };
        std::fs::write(
            outbox_dir.join("001-a.json"),
            serde_json::to_vec(&valid).unwrap(),
        )
        .unwrap();
        let orphan = OutboxMessage {
            to: "ghost-session".into(),
            from: None,
            body: "无人接收".into(),
        };
        std::fs::write(
            outbox_dir.join("002-b.json"),
            serde_json::to_vec(&orphan).unwrap(),
        )
        .unwrap();

        let mut recorded = Vec::new();
        let mut transport_id = String::new();
        assert_eq!(
            queue
                .ingest_outbox_with_identity(&manager, |message, message_id| {
                    transport_id = message_id.to_string();
                    recorded.push((message.to.clone(), message.body.clone()));
                    Ok(())
                })
                .unwrap(),
            1
        );
        assert_eq!(
            recorded,
            vec![(created.id.clone(), "请回归接口".to_string())]
        );

        let pending = queue.peek_next(&created.id).unwrap();
        assert_eq!(pending.id, transport_id);
        assert_eq!(pending.body, "请回归接口");
        assert!(queue.peek_next("ghost-session").is_none());
        assert!(std::fs::read_dir(&outbox_dir).unwrap().next().is_none());

        let _ = std::fs::remove_dir_all(root);
    }

    fn durable_delivery_fixture(name: &str) -> (PathBuf, SessionManager, SessionRecord, SessionActionQueue) {
        let root = std::env::temp_dir().join(format!("cc-durable-{name}-{}", uuid::Uuid::new_v4()));
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let record = sessions.create(NewSession {
            provider_id: "glm".into(), provider_name: "GLM".into(), cwd: ".".into(),
            yolo: false, auto_mode: false, project_id: None,
        }).unwrap();
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        (root, sessions, record, queue)
    }

    fn ready_fixture_session(sessions: &SessionManager, record: &SessionRecord, pty_id: u32) {
        sessions.attach_runtime(&record.id, pty_id).unwrap();
        sessions.apply_hook_value(&serde_json::json!({
            "session_id": record.claude_session_id, "hook_event_name": "SessionStart", "permission_mode": "default"
        })).unwrap();
    }

    #[test]
    fn broadcast_disk_failure_rejects_the_whole_batch_and_retry_publishes_once() {
        let (root, sessions, first, queue) = durable_delivery_fixture("broadcast-failure");
        let second = sessions.create(NewSession {
            provider_id: "glm".into(), provider_name: "GLM".into(), cwd: ".".into(),
            yolo: false, auto_mode: false, project_id: None,
        }).unwrap();
        ready_fixture_session(&sessions, &first, 1);
        ready_fixture_session(&sessions, &second, 2);
        let mailbox = root.join("mailbox.json");
        std::fs::create_dir(&mailbox).unwrap();
        let prompt = "broadcast must be durable for both recipients";
        for _ in 0..2 {
            let error = queue.enqueue_broadcast(prompt, &sessions.list()).unwrap_err();
            assert!(format!("{error:#}").contains("广播未入队"));
            assert!(queue.peek_next(&first.id).is_none());
            assert!(queue.peek_next(&second.id).is_none());
            assert!(queue.mailbox_error().unwrap().contains("广播未入队"));
            assert!(queue.flush_ready_with_writer_at(&sessions, Instant::now(), |_, _| {
                panic!("failed broadcast must not become deliverable")
            }).is_empty());
        }
        std::fs::remove_dir(&mailbox).unwrap();
        let receipt = queue.enqueue_broadcast(prompt, &sessions.list()).unwrap();
        assert_eq!(receipt.target_count, 2);
        assert!(queue.mailbox_error().is_none());
        drop(queue);
        let queue = SessionActionQueue::open_at(mailbox, root.join("outbox")).unwrap();
        for session_id in [&first.id, &second.id] {
            let items = queue.items.lock().unwrap();
            let messages = items.get(session_id).unwrap();
            assert_eq!(messages.len(), 1, "failed attempts must not leave duplicate broadcasts");
            assert_eq!(messages[0].body, prompt);
            assert_eq!(messages[0].kind, MessageKind::Broadcast);
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn direct_message_batch_disk_failure_cannot_partially_deliver_or_report_success() {
        let (root, _sessions, first, queue) = durable_delivery_fixture("direct-batch-failure");
        let mailbox = root.join("mailbox.json");
        std::fs::create_dir(&mailbox).unwrap();
        let batch = vec![
            (first.id.clone(), QueuedMessage::new("one".into(), None)),
            ("second".into(), QueuedMessage::new("two".into(), None)),
        ];
        assert!(queue.enqueue_messages_durable(batch.clone()).is_err());
        assert!(queue.items.lock().unwrap().is_empty());
        std::fs::remove_dir(&mailbox).unwrap();
        queue.enqueue_messages_durable(batch).unwrap();
        drop(queue);
        let reopened = SessionActionQueue::open_at(mailbox, root.join("outbox")).unwrap();
        assert_eq!(reopened.items.lock().unwrap()[&first.id].len(), 1);
        assert_eq!(reopened.peek_next("second").unwrap().body, "two");
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn human_message_disk_failure_is_not_deliverable_and_same_request_can_retry() {
        let (root, _sessions, record, queue) = durable_delivery_fixture("human-failure");
        let mailbox = root.join("mailbox.json");
        std::fs::create_dir(&mailbox).unwrap();
        let message = QueuedMessage::new("original message".into(), None);
        let id = message.id.clone();
        assert!(queue.enqueue_human_message(&record.id, Some("project"), "Target", message.clone(), true).is_err());
        assert!(queue.peek_next(&record.id).is_none());
        assert_eq!(queue.accepted_human_message(&id, &record.id, Some("project"), &message.body).unwrap(), None);
        std::fs::remove_dir(&mailbox).unwrap();
        queue.enqueue_human_message(&record.id, Some("project"), "Target", message.clone(), true).unwrap();
        assert_eq!(queue.items.lock().unwrap()[&record.id].len(), 1);
        assert_eq!(queue.accepted_human_message(&id, &record.id, Some("project"), &message.body).unwrap().as_deref(), Some("Target"));
        assert!(queue.mailbox_error().is_none());
        drop(queue);
        let queue = SessionActionQueue::open_at(mailbox.clone(), root.join("outbox")).unwrap();
        queue.enqueue_human_message(&record.id, Some("project"), "Renamed target", message.clone(), true).unwrap();
        assert_eq!(queue.items.lock().unwrap()[&record.id].len(), 1, "a lost IPC reply must not enqueue twice after restart");
        assert!(queue.commit_delivered_message(&record.id, &id));
        drop(queue);
        let queue = SessionActionQueue::open_at(mailbox, root.join("outbox")).unwrap();
        queue.enqueue_human_message(&record.id, Some("project"), "Target", message.clone(), true).unwrap();
        assert!(queue.peek_next(&record.id).is_none(), "a completed delivery retains its retry receipt");
        assert!(queue.accepted_human_message(&id, "another-session", Some("project"), &message.body).is_err());
        assert!(queue.accepted_human_message(&id, &record.id, Some("another-project"), &message.body).is_err());
        assert!(queue.accepted_human_message(&id, &record.id, Some("project"), "changed body").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn archive_keeps_human_receipts_and_permanent_deletion_reclaims_them() {
        let (root, sessions, record, queue) = durable_delivery_fixture("human-delete");
        let message = QueuedMessage::new("accepted before archive".into(), None);
        let id = message.id.clone();
        queue.enqueue_human_message(&record.id, None, "Target", message.clone(), true).unwrap();
        queue.discard(&record.id);
        sessions.archive(&record.id).unwrap();
        assert!(queue.accepted_human_message(&id, &record.id, None, &message.body).unwrap().is_some());
        drop(queue);
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        assert!(queue.accepted_human_message(&id, &record.id, None, &message.body).unwrap().is_some());
        sessions.delete(&record.id).unwrap();
        queue.forget_human_message_receipts(&record.id).unwrap();
        assert!(sessions.get(&record.id).is_none());
        drop(queue);
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        assert!(queue.human_message_receipts.lock().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn simultaneous_human_retries_publish_one_durable_message() {
        let (root, _sessions, record, queue) = durable_delivery_fixture("human-concurrent");
        let message = QueuedMessage::new("same request".into(), None);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                let queue = &queue;
                let session_id = &record.id;
                let message = message.clone();
                scope.spawn(move || queue.enqueue_human_message(session_id, Some("project"), "Target", message, true).unwrap());
            }
        });
        assert_eq!(queue.items.lock().unwrap()[&record.id].len(), 1);
        drop(queue);
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        assert_eq!(queue.items.lock().unwrap()[&record.id].len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn outbox_disk_failure_keeps_source_and_retries_once_without_consuming_quota() {
        let (root, sessions, record, queue) = durable_delivery_fixture("outbox-failure");
        let mailbox = root.join("mailbox.json");
        std::fs::create_dir(&mailbox).unwrap();
        let source = root.join("outbox/stable-source.json");
        let raw = serde_json::to_vec(&OutboxMessage {
            to: record.id.clone(), body: "must survive".into(),
            from: Some(MessageOrigin { session_id: "sender".into(), label: "Sender".into() }),
        }).unwrap();
        std::fs::write(&source, &raw).unwrap();
        let mut callbacks = Vec::new();
        for _ in 0..AGENT_MESSAGE_PAIR_LIMIT + 2 {
            assert!(queue.ingest_outbox_with_identity(&sessions, |_, id| { callbacks.push(id.to_string()); Ok(()) }).is_err());
            assert!(source.exists());
            assert!(queue.peek_next(&record.id).is_none(), "failed persistence cannot expose a deliverable in-memory message");
        }
        assert!(callbacks.iter().all(|id| id == &callbacks[0]), "retries must keep the collaboration store ID stable");
        assert!(queue.mailbox_error().is_some());
        assert!(queue.agent_message_log.lock().unwrap().values().all(VecDeque::is_empty));
        std::fs::remove_dir(&mailbox).unwrap();
        assert_eq!(queue.ingest_outbox_with_identity(&sessions, |_, id| { assert_eq!(id, callbacks[0]); Ok(()) }).unwrap(), 1);
        let message_id = queue.peek_next(&record.id).unwrap().id;
        assert!(!source.exists());
        assert!(queue.mailbox_error().is_none());
        assert_eq!(queue.agent_message_log.lock().unwrap().values().map(VecDeque::len).sum::<usize>(), 1);
        drop(queue);
        // Simulate a source cleanup failure/crash after durable enqueue.
        std::fs::write(&source, &raw).unwrap();
        let queue = SessionActionQueue::open_at(mailbox.clone(), root.join("outbox")).unwrap();
        assert_eq!(queue.ingest_outbox_with_identity(&sessions, |_, _| panic!("already ingested")).unwrap(), 0);
        assert_eq!(queue.items.lock().unwrap()[&record.id].len(), 1);
        assert!(queue.commit_delivered_message(&record.id, &message_id));
        drop(queue);
        // Even if delivery finished before stale source cleanup, restart must not
        // enqueue the same transport a second time.
        std::fs::write(&source, &raw).unwrap();
        let queue = SessionActionQueue::open_at(mailbox, root.join("outbox")).unwrap();
        assert_eq!(queue.ingest_outbox_with_identity(&sessions, |_, _| panic!("duplicate collaboration record")).unwrap(), 0);
        assert!(queue.peek_next(&record.id).is_none());
        assert!(!source.exists());
        queue.ingest_outbox(&sessions, |_| Ok(())).unwrap();
        assert!(queue.processed_outbox_ids.lock().unwrap().is_empty(), "cleaned sources must not accumulate receipts forever");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failing_ingress_callback_does_not_burn_agent_pair_quota() {
        let (root, sessions, record, queue) = durable_delivery_fixture("callback-retry");
        let source = root.join("outbox/callback-source.json");
        std::fs::write(&source, serde_json::to_vec(&OutboxMessage {
            to: record.id.clone(), body: "retry collaboration storage".into(),
            from: Some(MessageOrigin { session_id: "sender".into(), label: "Sender".into() }),
        }).unwrap()).unwrap();
        let mut ids = HashSet::new();
        for _ in 0..AGENT_MESSAGE_PAIR_LIMIT + 2 {
            assert_eq!(queue.ingest_outbox_with_identity(&sessions, |_, id| { ids.insert(id.to_string()); Err(anyhow!("fixture store unavailable")) }).unwrap(), 0);
        }
        assert_eq!(ids.len(), 1);
        assert!(source.exists());
        assert!(queue.peek_next(&record.id).is_none());
        assert_eq!(queue.ingest_outbox(&sessions, |_| Ok(())).unwrap(), 1);
        assert_eq!(queue.items.lock().unwrap()[&record.id].len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn message_written_without_enter_survives_restart_and_is_submitted_after_restore() {
        let (root, sessions, record, queue) = durable_delivery_fixture("restart-before-enter");
        ready_fixture_session(&sessions, &record, 1);
        let message = QueuedMessage::new("durable text".into(), None);
        queue.enqueue_message(&record.id, message.clone());
        let now = Instant::now();
        let mut first = Vec::new();
        assert_eq!(queue.flush_ready_with_writer_at(&sessions, now, |_, bytes| { first.push(bytes.to_vec()); Ok(()) }), vec![record.id.clone()]);
        assert_eq!(first.len(), 1);
        assert_eq!(queue.peek_next(&record.id).unwrap().id, message.id);
        drop(queue);
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        ready_fixture_session(&sessions, &record, 2);
        let mut restored = Vec::new();
        queue.flush_ready_with_writer_at(&sessions, now, |pty_id, bytes| { assert_eq!(pty_id, 2); restored.push(bytes.to_vec()); Ok(()) });
        assert_eq!(restored, first, "restart must rewrite the complete pending text");
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |pty_id, bytes| { assert_eq!(pty_id, 2); restored.push(bytes.to_vec()); Ok(()) });
        assert_eq!(restored.last().unwrap(), b"\r");
        assert!(queue.peek_next(&record.id).is_none());
        drop(queue);
        assert!(SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap().peek_next(&record.id).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn process_exit_before_enter_keeps_message_until_a_new_pty_is_ready() {
        let (root, sessions, record, queue) = durable_delivery_fixture("exit-before-enter");
        ready_fixture_session(&sessions, &record, 1);
        let message = QueuedMessage::new("keep after process exit".into(), None);
        queue.enqueue_message(&record.id, message.clone());
        let now = Instant::now();
        queue.flush_ready_with_writer_at(&sessions, now, |_, _| Ok(()));
        sessions.mark_stopped(&record.id).unwrap();
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| panic!("cannot submit to exited PTY"));
        assert!(!queue.pending_submits.lock().unwrap().contains_key(&record.id));
        assert_eq!(queue.peek_next(&record.id).unwrap().id, message.id);
        assert_eq!(queue.delivery_records()[0].stage, "queued");
        assert!(queue.delivery_records()[0].error.is_some());
        ready_fixture_session(&sessions, &record, 2);
        assert_eq!(queue.flush_ready_with_writer_at(&sessions, now, |id, _| { assert_eq!(id, 2); Ok(()) }), vec![record.id.clone()]);
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |id, bytes| { assert_eq!(id, 2); assert_eq!(bytes, b"\r"); Ok(()) });
        assert!(queue.peek_next(&record.id).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manual_enter_acknowledges_pending_text_without_automatic_resubmit() {
        let (root, sessions, record, queue) = durable_delivery_fixture("manual-enter");
        ready_fixture_session(&sessions, &record, 1);
        queue.enqueue_message(&record.id, QueuedMessage::new("manually submitted".into(), None));
        let now = Instant::now();
        queue.flush_ready_with_writer_at(&sessions, now, |_, _| Ok(()));
        queue.cancel_pending_submit(&record.id); // The caller has already written Enter.
        assert!(queue.peek_next(&record.id).is_none());
        assert!(!queue.pending_submits.lock().unwrap().contains_key(&record.id));
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| panic!("manual Enter already submitted"));
        assert_eq!(queue.delivery_records()[0].stage, "submit_sent");
        assert!(SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap().peek_next(&record.id).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn permission_prompt_between_paste_and_enter_never_receives_automatic_confirmation() {
        let (root, sessions, record, queue) = durable_delivery_fixture("permission-before-enter");
        ready_fixture_session(&sessions, &record, 1);
        let message = QueuedMessage::new("message must wait for human approval".into(), None);
        queue.enqueue_message(&record.id, message.clone());
        let now = Instant::now();
        assert_eq!(queue.flush_ready_with_writer_at(&sessions, now, |_, _| Ok(())), vec![record.id.clone()]);
        sessions.apply_hook_value(&serde_json::json!({
            "session_id": record.claude_session_id,
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt",
            "message": "needs explicit approval"
        })).unwrap();
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Waiting);
        for _ in 0..2 {
            queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| {
                panic!("a queued message must never approve a permission prompt")
            });
        }
        assert_eq!(queue.peek_next(&record.id).unwrap().id, message.id);
        assert!(queue.pending_submits.lock().unwrap().contains_key(&record.id));
        let evidence = queue.delivery_records();
        assert_eq!(evidence[0].stage, "text_written");
        assert!(evidence[0].error.as_ref().unwrap().contains("等待人工处理"));
        assert_eq!(SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap().peek_next(&record.id).unwrap().id, message.id);

        sessions.apply_hook_value(&serde_json::json!({
            "session_id": record.claude_session_id,
            "hook_event_name": "PostToolUse"
        })).unwrap();
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| {
            panic!("resolving approval is not the same as returning to an input prompt")
        });
        sessions.apply_hook_value(&serde_json::json!({
            "session_id": record.claude_session_id,
            "hook_event_name": "SessionStart",
            "permission_mode": "default"
        })).unwrap();
        let mut enters = 0;
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, bytes| {
            assert_eq!(bytes, b"\r");
            enters += 1;
            Ok(())
        });
        assert_eq!(enters, 1);
        assert!(queue.peek_next(&record.id).is_none());
        let evidence = queue.delivery_records();
        assert_eq!(evidence[0].stage, "submit_sent");
        assert!(evidence[0].error.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn fast_manual_approval_before_the_first_flush_waits_for_a_fresh_input_prompt() {
        let (root, sessions, record, queue) = durable_delivery_fixture("fast-approval-before-enter");
        ready_fixture_session(&sessions, &record, 1);
        let message = QueuedMessage::new("deliver after the previous operation completes".into(), None);
        queue.enqueue_message(&record.id, message.clone());
        let now = Instant::now();
        queue.flush_ready_with_writer_at(&sessions, now, |_, _| Ok(()));
        sessions.apply_hook_value(&serde_json::json!({
            "session_id": record.claude_session_id,
            "hook_event_name": "Notification",
            "notification_type": "permission_prompt"
        })).unwrap();
        // This is the pty_input sequence for an explicit approval. Waiting is
        // already gone by the first delayed-submit heartbeat.
        queue.hold_pending_submit_for_blocker(&record.id);
        sessions.mark_terminal_submission(1).unwrap();
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
        assert!(!sessions.is_ready_for_prompt(&record.id));
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| {
            panic!("human approval must not submit the waiting chat message")
        });
        assert_eq!(queue.peek_next(&record.id).unwrap().id, message.id);
        assert_eq!(queue.delivery_records()[0].stage, "text_written");
        sessions.apply_hook_value(&serde_json::json!({
            "session_id": record.claude_session_id,
            "hook_event_name": "Stop"
        })).unwrap();
        let mut enters = 0;
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, bytes| {
            assert_eq!(bytes, b"\r");
            enters += 1;
            Ok(())
        });
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| {
            panic!("the message must submit exactly once")
        });
        assert_eq!(enters, 1);
        assert!(queue.peek_next(&record.id).is_none());
        assert_eq!(queue.delivery_records()[0].stage, "submit_sent");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_update_dialog_after_paste_blocks_enter_until_a_fresh_composer() {
        let (root, sessions, _unused_record, queue) = durable_delivery_fixture("codex-update-before-enter");
        let record = sessions.create_runtime(NewRuntimeSession {
            runtime_kind: "codex".into(), cwd: ".".into(), project_id: None,
        }).unwrap();
        sessions.attach_runtime(&record.id, 31).unwrap();
        let observe = |output: &str| {
            sessions.observe_runtime_prompt_output_with_blocker(
                &record.id, output,
                |text| crate::multi_agent::interactive_runtime_prompt_ready(
                    crate::multi_agent::AgentRuntimeKind::Codex, text,
                ),
                crate::multi_agent::interactive_runtime_prompt_blocked,
            )
        };
        let composer = "\x1b[2J\x1b[HOpenAI Codex\n› \n/model to change";
        assert!(observe(composer));
        let message = QueuedMessage::new("must not select Update now".into(), None);
        queue.enqueue_message(&record.id, message.clone());
        let now = Instant::now();
        assert_eq!(queue.flush_ready_with_writer_at(&sessions, now, |pty, bytes| {
            assert_eq!(pty, 31);
            assert!(bytes.starts_with(b"\x1b[200~"));
            Ok(())
        }), vec![record.id.clone()]);
        assert!(!observe("Update available! 0.162.0 -> 0.163.0\n1. Update now\n2. Skip\nenter continue · esc skip"));
        assert_eq!(sessions.get(&record.id).unwrap().status, SessionStatus::Running);
        assert!(sessions.runtime_prompt_is_blocked(&record.id));
        sessions.mark_prompt_dispatched(&record.id);
        assert!(sessions.runtime_prompt_is_blocked(&record.id));
        for _ in 0..2 {
            queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| {
                panic!("message delivery must not select the CLI's update action")
            });
        }
        assert_eq!(queue.peek_next(&record.id).unwrap().id, message.id);
        assert_eq!(queue.delivery_records()[0].stage, "text_written");
        assert!(queue.delivery_records()[0].error.as_ref().unwrap().contains("等待人工处理"));
        assert!(!observe("\r\nredraw fragment without a ready composer"));
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| {
            panic!("a partial redraw cannot dismiss the update dialog")
        });
        assert!(observe(composer));
        assert!(!sessions.runtime_prompt_is_blocked(&record.id));
        let mut enters = 0;
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |pty, bytes| {
            assert_eq!(pty, 31);
            assert_eq!(bytes, b"\r");
            enters += 1;
            Ok(())
        });
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| {
            panic!("one message must only submit once")
        });
        assert_eq!(enters, 1);
        assert!(queue.peek_next(&record.id).is_none());
        assert!(queue.delivery_records()[0].error.is_none());
        assert!(SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap().peek_next(&record.id).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn successful_enter_with_failed_durable_ack_is_not_sent_twice_in_process() {
        let (root, sessions, record, queue) = durable_delivery_fixture("ack-failure");
        ready_fixture_session(&sessions, &record, 1);
        let message = QueuedMessage::new("one submit".into(), None);
        queue.enqueue_message(&record.id, message.clone());
        let now = Instant::now();
        queue.flush_ready_with_writer_at(&sessions, now, |_, _| Ok(()));
        let mailbox = root.join("mailbox.json");
        std::fs::remove_file(&mailbox).unwrap();
        std::fs::create_dir(&mailbox).unwrap();
        let mut enters = 0;
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, bytes| { assert_eq!(bytes, b"\r"); enters += 1; Ok(()) });
        assert_eq!(enters, 1);
        assert_eq!(queue.peek_next(&record.id).unwrap().id, message.id);
        assert!(queue.mailbox_error().unwrap().contains("确认落盘失败"));
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| panic!("Enter was already sent"));
        std::fs::remove_dir(&mailbox).unwrap();
        queue.flush_pending_submits_with_writer_at(&sessions, now + SUBMIT_DELAY, |_, _| panic!("only durable acknowledgement is retried"));
        assert!(queue.peek_next(&record.id).is_none());
        assert!(queue.mailbox_error().is_none());
        assert!(!queue.pending_submits.lock().unwrap().contains_key(&record.id));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn flush_ready_keeps_message_when_pty_write_fails() {
        let root = std::env::temp_dir().join(format!("crewvela-flush-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        // 挂上运行中但不存在的 PTY id：write 必失败，消息必须留在队首。
        manager.attach_runtime(&created.id, 4242).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart"
            }))
            .unwrap();
        let queue = SessionActionQueue::new();
        queue.enqueue_message(&created.id, QueuedMessage::new("回归一下".into(), None));

        assert!(queue.flush_ready(&manager, &PtyManager::new()).is_empty());

        let pending = queue.peek_next(&created.id).unwrap();
        assert_eq!(pending.body, "回归一下");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delivered_text_writes_without_enter_and_submit_follows_after_delay() {
        let root = std::env::temp_dir().join(format!("crewvela-submit-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 7).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart"
            }))
            .unwrap();
        let queue = SessionActionQueue::new();
        let message = QueuedMessage::new(
            "帮忙回归接口".into(),
            Some(MessageOrigin {
                session_id: "sender-1".into(),
                label: "接口检查 (Kimi-K2)".into(),
            }),
        );
        let expected_text = format!("\x1b[200~{}\x1b[201~", render_delivery(&message));
        queue.enqueue_message(&created.id, message);

        let writes = std::cell::RefCell::new(Vec::<(u32, Vec<u8>)>::new());
        let mut writer = |pty_id: u32, bytes: &[u8]| {
            writes.borrow_mut().push((pty_id, bytes.to_vec()));
            Ok(())
        };
        let t0 = Instant::now();

        // 文本单独写出，不带 \r。
        let dispatched = queue.flush_ready_with_writer_at(&manager, t0, &mut writer);
        assert_eq!(dispatched, vec![created.id.clone()]);
        assert_eq!(
            writes.borrow().as_slice(),
            &[(7u32, expected_text.into_bytes())]
        );

        // 未到 SUBMIT_DELAY 不补回车；到点后补一次 \r 且只补一次。
        queue.flush_pending_submits_with_writer_at(
            &manager,
            t0 + Duration::from_millis(100),
            &mut writer,
        );
        assert_eq!(writes.borrow().len(), 1);
        queue.flush_pending_submits_with_writer_at(&manager, t0 + SUBMIT_DELAY, &mut writer);
        assert_eq!(writes.borrow().as_slice()[1], (7u32, b"\r".to_vec()));
        queue.flush_pending_submits_with_writer_at(
            &manager,
            t0 + SUBMIT_DELAY + Duration::from_millis(50),
            &mut writer,
        );
        assert_eq!(writes.borrow().len(), 2);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delivery_log_records_chat_delivery_and_skips_initial_prompt() {
        let root = std::env::temp_dir().join(format!("crewvela-delivery-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let chat_target = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        let launch_target = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&chat_target.id, 7).unwrap();
        manager.attach_runtime(&launch_target.id, 8).unwrap();
        for session in [&chat_target, &launch_target] {
            manager
                .apply_hook_value(&serde_json::json!({
                    "session_id": session.claude_session_id,
                    "hook_event_name": "SessionStart"
                }))
                .unwrap();
        }
        let queue = SessionActionQueue::new();
        queue.enqueue_many(&[launch_target.id.clone()], "启动任务不算协作".into());
        queue.enqueue_message(
            &chat_target.id,
            QueuedMessage::new(
                "聊聊接口".into(),
                Some(MessageOrigin {
                    session_id: "sender-1".into(),
                    label: "接口检查".into(),
                }),
            ),
        );

        let mut writer = |_: u32, _: &[u8]| Ok(());
        let dispatched = queue.flush_ready_with_writer_at(&manager, Instant::now(), &mut writer);

        assert_eq!(dispatched.len(), 2);
        let records = queue.delivery_records();
        assert_eq!(records.len(), 1, "初始任务不进投影，只记录 chat 投递");
        assert_eq!(records[0].to_session_id, chat_target.id);
        assert_eq!(records[0].body, "聊聊接口");
        assert_eq!(records[0].kind, MessageKind::Chat);
        assert_eq!(records[0].from_session_id.as_deref(), Some("sender-1"));
        assert_eq!(records[0].from_label.as_deref(), Some("接口检查"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delivery_log_marks_broadcast_and_system_kinds() {
        let root =
            std::env::temp_dir().join(format!("crewvela-delivery-k-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let broadcast_target = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        let notice_target = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&broadcast_target.id, 7).unwrap();
        manager.attach_runtime(&notice_target.id, 8).unwrap();
        for session in [&broadcast_target, &notice_target] {
            manager
                .apply_hook_value(&serde_json::json!({
                    "session_id": session.claude_session_id,
                    "hook_event_name": "SessionStart"
                }))
                .unwrap();
        }
        let queue = SessionActionQueue::new();
        // create 返回的是 attach 前的快照，pty_id 仍是 None；从 manager 重取最新状态。
        let live_broadcast_target = manager.get(&broadcast_target.id).unwrap();
        queue
            .enqueue_broadcast("统一检查", std::slice::from_ref(&live_broadcast_target))
            .unwrap();
        queue.enqueue_message(
            &notice_target.id,
            QueuedMessage::with_kind("已达上限".into(), None, MessageKind::System),
        );

        let mut writer = |_: u32, _: &[u8]| Ok(());
        queue.flush_ready_with_writer_at(&manager, Instant::now(), &mut writer);

        let records = queue.delivery_records();
        let kinds: Vec<MessageKind> = records.iter().map(|record| record.kind).collect();
        assert!(
            kinds.contains(&MessageKind::Broadcast),
            "广播应标 Broadcast: {kinds:?}"
        );
        assert!(
            kinds.contains(&MessageKind::System),
            "系统通知应标 System: {kinds:?}"
        );
        let notice = records
            .iter()
            .find(|record| record.kind == MessageKind::System)
            .unwrap();
        assert_eq!(notice.to_session_id, notice_target.id);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn delivery_log_is_capped_and_keeps_newest() {
        let queue = SessionActionQueue::new();
        for index in 0..505 {
            queue.record_delivery(
                "session-x",
                &QueuedMessage::new(format!("消息-{index}"), None),
            );
        }

        let records = queue.delivery_records();
        assert_eq!(records.len(), 500);
        assert_eq!(records[0].body, "消息-5", "最旧的 5 条应被挤出");
        assert_eq!(records[499].body, "消息-504");
    }

    #[test]
    fn pending_submit_dropped_when_session_gone() {
        let root =
            std::env::temp_dir().join(format!("crewvela-submit-gone-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let queue = SessionActionQueue::new();
        queue
            .pending_submits
            .lock()
            .unwrap()
            .insert("ghost".to_string(), PendingSubmit { due: Instant::now(), message_id: "ghost-message".into(), pty_id: 0, submitted: false, awaiting_prompt_after_blocker: false });

        let mut wrote = false;
        queue.flush_pending_submits_with_writer_at(
            &manager,
            Instant::now() + SUBMIT_DELAY,
            |_, _| {
                wrote = true;
                Ok(())
            },
        );

        assert!(!wrote);
        assert!(queue.pending_submits.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn agent_message_pair_limit_allows_ten_and_blocks_eleventh_with_one_notice() {
        let root = std::env::temp_dir().join(format!("crewvela-limit-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let target = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        let outbox_dir = root.join("outbox");
        let queue =
            SessionActionQueue::open_at(root.join("mailbox.json"), outbox_dir.clone()).unwrap();
        let write_outbox = |suffix: &str| {
            let message = OutboxMessage {
                to: target.id.clone(),
                from: Some(MessageOrigin {
                    session_id: "sender-1".into(),
                    label: "接口检查 (Kimi-K2)".into(),
                }),
                body: format!("第 {suffix} 条"),
            };
            std::fs::write(
                outbox_dir.join(format!("{suffix}.json")),
                serde_json::to_string(&message).unwrap(),
            )
            .unwrap();
        };

        for index in 0..AGENT_MESSAGE_PAIR_LIMIT {
            write_outbox(&format!("{index:03}"));
        }
        assert_eq!(
            queue.ingest_outbox(&manager, |_| Ok(())).unwrap(),
            AGENT_MESSAGE_PAIR_LIMIT
        );

        // 第 11 条：不投递给目标，转而给发送方回发一次停止通知。
        write_outbox("011");
        assert_eq!(queue.ingest_outbox(&manager, |_| Ok(())).unwrap(), 0);
        let notice = queue.take_next("sender-1").unwrap();
        assert!(notice.from.is_none());
        assert!(notice.body.contains("已达上限"));
        assert!(notice.body.contains(&target.title));
        assert!(queue.peek_next(&target.id).is_some());

        // 第 12 条：仍丢弃，但同一窗口内不再重复通知（防止通知回路）。
        write_outbox("012");
        assert_eq!(queue.ingest_outbox(&manager, |_| Ok(())).unwrap(), 0);
        assert!(queue.take_next("sender-1").is_none());

        // 目标队列正好 10 条，双向合计口径（from/to 颠倒共享同一配额）。
        for _ in 0..AGENT_MESSAGE_PAIR_LIMIT {
            assert!(queue.take_next(&target.id).is_some());
        }
        assert!(queue.take_next(&target.id).is_none());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn agent_message_pair_limit_window_slides_and_counts_both_directions() {
        let queue = SessionActionQueue::new();
        let t0 = Instant::now();

        for offset in 0..AGENT_MESSAGE_PAIR_LIMIT {
            let (from, to) = if offset % 2 == 0 {
                ("a", "b")
            } else {
                ("b", "a") // 反向共享同一对配额
            };
            assert!(queue.record_agent_message_at(
                from,
                to,
                t0 + Duration::from_secs(offset as u64)
            ));
        }
        assert!(!queue.record_agent_message_at("a", "b", t0 + Duration::from_secs(11)));
        // 窗口滑过后旧记录出局，可以再次发送。
        assert!(queue.record_agent_message_at(
            "b",
            "a",
            t0 + AGENT_MESSAGE_PAIR_WINDOW + Duration::from_secs(1)
        ));
    }

    #[test]
    fn human_directed_message_resets_quota_for_that_session_only() {
        let queue = SessionActionQueue::new();
        let t0 = Instant::now();
        for offset in 0..AGENT_MESSAGE_PAIR_LIMIT {
            assert!(queue.record_agent_message_at(
                "a",
                "b",
                t0 + Duration::from_secs(offset as u64)
            ));
            assert!(queue.record_agent_message_at(
                "c",
                "d",
                t0 + Duration::from_secs(offset as u64)
            ));
        }
        assert!(!queue.record_agent_message_at("a", "b", t0 + Duration::from_secs(20)));

        // 人 @ 了 a：a 参与的对全部豁免，无关的 c↔d 不受影响。
        queue.reset_agent_quota_involving("a");
        assert!(queue.record_agent_message_at("b", "a", t0 + Duration::from_secs(21)));
        assert!(!queue.record_agent_message_at("c", "d", t0 + Duration::from_secs(22)));

        // 通知去重状态同样被重置（a↔b 超限时可以再次通知）。
        queue.enqueue_agent_limit_notice(
            "a",
            "b",
            "乙",
            AGENT_MESSAGE_PAIR_LIMIT,
            t0 + Duration::from_secs(30),
        );
        assert!(queue.take_next("a").is_some());
        for _ in 0..AGENT_MESSAGE_PAIR_LIMIT {
            queue.record_agent_message_at("a", "b", t0 + Duration::from_secs(31));
        }
        queue.enqueue_agent_limit_notice(
            "a",
            "b",
            "乙",
            AGENT_MESSAGE_PAIR_LIMIT,
            t0 + Duration::from_secs(32),
        );
        assert!(queue.take_next("a").is_none()); // 同窗口第二次不重复通知
        queue.reset_agent_quota_involving("a");
        queue.enqueue_agent_limit_notice(
            "a",
            "b",
            "乙",
            AGENT_MESSAGE_PAIR_LIMIT,
            t0 + Duration::from_secs(33),
        );
        assert!(queue.take_next("a").is_some()); // 重置后恢复可通知
    }

    #[test]
    fn prompt_queue_keeps_one_fifo_per_business_session() {
        let queue = SessionActionQueue::new();
        queue.enqueue_many(
            &["session-a".into(), "session-b".into()],
            "检查构建结果".into(),
        );
        queue.enqueue_many(&["session-a".into()], "提交代码".into());

        assert_eq!(
            queue.take_next("session-a").map(|message| message.body),
            Some("检查构建结果".to_string())
        );
        assert_eq!(
            queue.take_next("session-a").map(|message| message.body),
            Some("提交代码".to_string())
        );
        assert_eq!(
            queue.take_next("session-b").map(|message| message.body),
            Some("检查构建结果".to_string())
        );
        assert!(queue.take_next("session-b").is_none());
    }

    #[test]
    fn replacing_control_launch_batch_discards_the_previous_batch() {
        let batch = ControlWorkspaceTrustBatch::new();
        batch.replace(vec!["old".into()]);
        batch.replace(vec!["new-a".into(), "new-b".into(), "new-a".into()]);

        assert_eq!(batch.session_ids(), vec!["new-a", "new-b"]);
    }

    #[test]
    fn empty_control_launch_batch_clears_previous_sessions() {
        let batch = ControlWorkspaceTrustBatch::new();
        batch.replace(vec!["old".into()]);
        batch.replace(Vec::new());

        assert!(batch.session_ids().is_empty());
    }

    #[test]
    fn broadcast_targets_every_live_business_session_and_skips_stopped_ones() {
        let queue = SessionActionQueue::new();
        let receipt = queue
            .enqueue_broadcast(
                "统一检查项目",
                &[
                    session("live-a", Some(1)),
                    session("stopped", None),
                    terminal_session("terminal", Some(3)),
                    session("live-b", Some(2)),
                ],
            )
            .unwrap();

        assert_eq!(receipt.target_count, 2);
        assert_eq!(
            receipt
                .targets
                .iter()
                .map(|target| target.session_id.as_str())
                .collect::<Vec<_>>(),
            vec!["live-a", "live-b"]
        );
        assert!(queue.take_next("terminal").is_none());
    }

    #[test]
    fn broadcast_preserves_prompt_verbatim() {
        let queue = SessionActionQueue::new();

        queue
            .enqueue_broadcast("  第一行\n第二行  ", &[session("live", Some(1))])
            .unwrap();

        assert_eq!(
            queue.take_next("live").map(|message| message.body),
            Some("  第一行\n第二行  ".to_string())
        );
    }

    #[test]
    fn permission_mode_switch_steps_reach_exact_target() {
        use crate::sessions::PermissionMode;

        assert_eq!(
            permission_mode_steps(PermissionMode::Default, PermissionMode::Auto, false),
            Some(3)
        );
        assert_eq!(
            permission_mode_steps(PermissionMode::Auto, PermissionMode::AcceptEdits, false),
            Some(2)
        );
        assert_eq!(
            permission_mode_steps(PermissionMode::Plan, PermissionMode::Auto, true),
            Some(2)
        );
        assert_eq!(
            permission_mode_steps(PermissionMode::DontAsk, PermissionMode::Auto, false),
            None
        );
    }

    #[test]
    fn multi_step_permission_mode_change_writes_one_backtab_per_interval() {
        let root = std::env::temp_dir().join(format!("crewvela-mode-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 42).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart",
                "permission_mode": "default"
            }))
            .unwrap();

        let queue = SessionActionQueue::new();
        queue
            .enqueue_permission_mode(PermissionMode::Auto, &manager.list())
            .unwrap();

        let started_at = Instant::now();
        let writes = std::cell::RefCell::new(Vec::<Vec<u8>>::new());
        let mut writer = |_: u32, bytes: &[u8]| {
            writes.borrow_mut().push(bytes.to_vec());
            Ok(())
        };

        queue.flush_permission_modes_with_writer_at(&manager, started_at, &mut writer);
        assert_eq!(*writes.borrow(), vec![BACKTAB.to_vec()]);
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart",
                "permission_mode": "acceptEdits"
            }))
            .unwrap();

        queue.flush_permission_modes_with_writer_at(
            &manager,
            started_at + MODE_STEP_DELAY / 2,
            &mut writer,
        );
        assert_eq!(*writes.borrow(), vec![BACKTAB.to_vec()]);

        queue.flush_permission_modes_with_writer_at(
            &manager,
            started_at + MODE_STEP_DELAY,
            &mut writer,
        );
        assert_eq!(*writes.borrow(), vec![BACKTAB.to_vec(), BACKTAB.to_vec()]);
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart",
                "permission_mode": "plan"
            }))
            .unwrap();

        queue.flush_permission_modes_with_writer_at(
            &manager,
            started_at + MODE_STEP_DELAY * 2,
            &mut writer,
        );
        assert_eq!(
            *writes.borrow(),
            vec![BACKTAB.to_vec(), BACKTAB.to_vec(), BACKTAB.to_vec()]
        );
        queue.flush_permission_modes_with_writer_at(
            &manager,
            started_at + MODE_STEP_DELAY * 3,
            &mut writer,
        );
        assert_eq!(
            *writes.borrow(),
            vec![BACKTAB.to_vec(), BACKTAB.to_vec(), BACKTAB.to_vec()]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn permission_mode_change_targets_every_live_session() {
        let queue = SessionActionQueue::new();
        let receipt = queue
            .enqueue_permission_mode(
                PermissionMode::Auto,
                &[
                    session("live-a", Some(1)),
                    session("stopped", None),
                    session("live-b", Some(2)),
                ],
            )
            .unwrap();

        assert_eq!(receipt.target_mode, PermissionMode::Auto);
        assert_eq!(receipt.target_count, 2);
        assert_eq!(queue.pending_mode("live-a"), Some(PermissionMode::Auto));
        assert_eq!(queue.pending_mode("live-b"), Some(PermissionMode::Auto));
        assert_eq!(queue.pending_mode("stopped"), None);
    }

    #[test]
    fn permission_mode_change_rejects_a_different_target_while_keys_are_still_queued() {
        let queue = SessionActionQueue::new();
        let sessions = [session("live-a", Some(1)), session("live-b", Some(2))];
        queue
            .enqueue_permission_mode(PermissionMode::Auto, &sessions)
            .unwrap();

        let error = queue
            .enqueue_permission_mode(PermissionMode::Plan, &sessions)
            .unwrap_err();

        assert!(error.to_string().contains("正在执行"));
        assert_eq!(queue.pending_mode("live-a"), Some(PermissionMode::Auto));
        assert_eq!(queue.pending_mode("live-b"), Some(PermissionMode::Auto));
    }

    #[test]
    fn permission_mode_can_change_again_without_a_hook_round_trip() {
        let root = std::env::temp_dir().join(format!("crewvela-mode-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 42).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart",
                "permission_mode": "default"
            }))
            .unwrap();

        let queue = SessionActionQueue::new();
        let writes = std::cell::RefCell::new(Vec::new());
        let mut writer = |_: u32, bytes: &[u8]| {
            writes.borrow_mut().push(bytes.to_vec());
            Ok(())
        };
        let started_at = Instant::now();

        queue
            .enqueue_permission_mode(PermissionMode::Plan, &manager.list())
            .unwrap();
        queue.flush_permission_modes_with_writer_at(&manager, started_at, &mut writer);
        queue.flush_permission_modes_with_writer_at(
            &manager,
            started_at + MODE_STEP_DELAY,
            &mut writer,
        );

        assert_eq!(
            manager.get(&created.id).unwrap().permission_mode,
            Some(PermissionMode::Plan)
        );
        assert_eq!(queue.pending_mode(&created.id), None);

        queue
            .enqueue_permission_mode(PermissionMode::Auto, &manager.list())
            .unwrap();
        queue.flush_permission_modes_with_writer_at(
            &manager,
            started_at + MODE_STEP_DELAY * 2,
            &mut writer,
        );

        assert_eq!(
            manager.get(&created.id).unwrap().permission_mode,
            Some(PermissionMode::Auto)
        );
        assert_eq!(queue.pending_mode(&created.id), None);
        assert_eq!(
            *writes.borrow(),
            vec![BACKTAB.to_vec(), BACKTAB.to_vec(), BACKTAB.to_vec()]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn queued_mode_change_is_not_dropped_when_reported_mode_becomes_unswitchable() {
        let root = std::env::temp_dir().join(format!("crewvela-mode-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 7).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart",
                "permission_mode": "default"
            }))
            .unwrap();

        let queue = SessionActionQueue::new();
        queue
            .enqueue_permission_mode(PermissionMode::Auto, &manager.list())
            .unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "Stop",
                "permission_mode": "dontAsk"
            }))
            .unwrap();

        let dispatch = queue.flush_permission_modes(&manager, &PtyManager::new());

        assert_eq!(dispatch.already_count, 0);
        assert_eq!(dispatch.sent_count, 0);
        assert_eq!(queue.pending_mode(&created.id), Some(PermissionMode::Auto));

        queue
            .enqueue_broadcast("检查项目", &manager.list())
            .unwrap();
        assert!(queue.flush_ready(&manager, &PtyManager::new()).is_empty());
        assert_eq!(
            queue.take_next(&created.id).map(|message| message.body),
            Some("检查项目".to_string())
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn muted_session_holds_agent_chat_but_delivers_human_messages() {
        let root = std::env::temp_dir().join(format!("crewvela-mute-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let created = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        manager.attach_runtime(&created.id, 51).unwrap();
        manager
            .apply_hook_value(&serde_json::json!({
                "session_id": created.claude_session_id,
                "hook_event_name": "SessionStart",
                "permission_mode": "default"
            }))
            .unwrap();
        let queue = SessionActionQueue::new();
        let writes = std::cell::RefCell::new(Vec::new());
        let rearm_readiness = |manager: &SessionManager, claude_session_id: &str| {
            queue.flush_pending_submits_with_writer_at(
                manager,
                Instant::now() + SUBMIT_DELAY,
                |_, _| Ok(()),
            );
            // 成功投递会撤掉 readiness（mark_prompt_dispatched），这里重新武装。
            manager
                .apply_hook_value(&serde_json::json!({
                    "session_id": claude_session_id,
                    "hook_event_name": "SessionStart",
                    "permission_mode": "default"
                }))
                .unwrap();
        };
        let flush = |queue: &SessionActionQueue| {
            queue.flush_ready_with_writer_at(&manager, Instant::now(), |_, bytes| {
                writes.borrow_mut().push(bytes.to_vec());
                Ok(())
            })
        };

        // 静音：agent 互发（带来源）暂存不投递。
        queue.set_activity(&created.id, AgentActivity::Muted);
        queue.enqueue_message(
            &created.id,
            QueuedMessage::new(
                "agent 消息".into(),
                Some(MessageOrigin {
                    session_id: "peer".into(),
                    label: "同伴".into(),
                }),
            ),
        );
        assert!(flush(&queue).is_empty());
        assert!(queue.peek_next(&created.id).is_some());

        // 队首为静音 Agent Chat 时，后面的人工插话仍能写入；原 Agent 消息保持原位。
        let held_id = queue.peek_next(&created.id).unwrap().id;
        queue.enqueue_message(&created.id, QueuedMessage::new("人工越过静音".into(), None));
        assert_eq!(flush(&queue), vec![created.id.clone()]);
        assert_eq!(queue.peek_next(&created.id).unwrap().id, held_id);
        assert!(String::from_utf8(writes.borrow()[0].clone())
            .unwrap()
            .contains("人工越过静音"));
        rearm_readiness(&manager, &created.claude_session_id);

        // 解除静音后按序补投。
        queue.set_activity(&created.id, AgentActivity::Active);
        assert_eq!(flush(&queue), vec![created.id.clone()]);
        assert_eq!(queue.peek_next(&created.id).unwrap().id, held_id, "text stays durable until Enter succeeds");
        queue.flush_pending_submits_with_writer_at(&manager, Instant::now() + SUBMIT_DELAY, |_, _| Ok(()));
        assert!(queue.peek_next(&created.id).is_none());
        assert_eq!(writes.borrow().len(), 2);

        // 静音下人工 @ 定向（from=None）照常投递。
        rearm_readiness(&manager, &created.claude_session_id);
        queue.set_activity(&created.id, AgentActivity::Muted);
        queue.enqueue_message(&created.id, QueuedMessage::new("人工指挥".into(), None));
        assert_eq!(flush(&queue), vec![created.id.clone()]);
        assert_eq!(writes.borrow().len(), 3);

        // 广播与初始任务同样不受静音影响。
        rearm_readiness(&manager, &created.claude_session_id);
        queue
            .enqueue_broadcast("全员注意", &[manager.list()[0].clone()])
            .unwrap();
        assert_eq!(flush(&queue), vec![created.id.clone()]);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn lurk_session_tightens_pair_limit_for_inbound_agent_messages() {
        let root = std::env::temp_dir().join(format!("crewvela-lurk-{}", uuid::Uuid::new_v4()));
        let manager =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let target = manager
            .create(NewSession {
                provider_id: "glm".into(),
                provider_name: "Z.AI GLM".into(),
                cwd: ".".into(),
                yolo: false,
                auto_mode: false,
                project_id: None,
            })
            .unwrap();
        let outbox_dir = root.join("outbox");
        let queue =
            SessionActionQueue::open_at(root.join("mailbox.json"), outbox_dir.clone()).unwrap();
        queue.set_activity(&target.id, AgentActivity::Lurk);

        let write_outbox = |suffix: &str| {
            let message = OutboxMessage {
                to: target.id.clone(),
                from: Some(MessageOrigin {
                    session_id: "sender-1".into(),
                    label: "发送方".into(),
                }),
                body: format!("第 {suffix} 条"),
            };
            std::fs::write(
                outbox_dir.join(format!("{suffix}.json")),
                serde_json::to_string(&message).unwrap(),
            )
            .unwrap();
        };

        // Lurk 限额 2 条：前两条投递，第 3 条丢弃并通知发送方，文案带实际限额。
        write_outbox("001");
        write_outbox("002");
        assert_eq!(queue.ingest_outbox(&manager, |_| Ok(())).unwrap(), 2);
        write_outbox("003");
        assert_eq!(queue.ingest_outbox(&manager, |_| Ok(())).unwrap(), 0);
        let notice = queue.take_next("sender-1").unwrap();
        assert!(notice.body.contains("2 条/小时"));

        // 队首仍是第 1 条（003 已被丢弃，不进队列）。
        assert_eq!(
            queue.peek_next(&target.id).map(|m| m.body),
            Some("第 001 条".to_string())
        );

        // 切回 Active 后配额恢复（窗口计数已含之前 2 条），004 入队；
        // 队列顺序为 001、002、004——被丢弃的 003 不占位。
        queue.set_activity(&target.id, AgentActivity::Active);
        write_outbox("004");
        assert_eq!(queue.ingest_outbox(&manager, |_| Ok(())).unwrap(), 1);
        let drained: Vec<String> = (0..3)
            .filter_map(|_| queue.take_next(&target.id).map(|m| m.body))
            .collect();
        assert_eq!(
            drained,
            vec![
                "第 001 条".to_string(),
                "第 002 条".to_string(),
                "第 004 条".to_string()
            ]
        );

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn activity_states_default_to_active_for_unknown_sessions() {
        let queue = SessionActionQueue::new();
        queue.set_activity("muted-one", AgentActivity::Muted);

        let states = queue.agent_activity_states(&["muted-one".into(), "other".into()]);
        assert_eq!(states.len(), 2);
        assert_eq!(states[0].activity, AgentActivity::Muted);
        assert_eq!(states[1].activity, AgentActivity::Active);
    }

    #[test]
    fn handoff_queue_is_idempotent_after_consumption_and_restart() {
        let root = std::env::temp_dir().join(format!("crewvela-handoff-{}", uuid::Uuid::new_v4()));
        let path = root.join("mailbox.json");
        let queue = SessionActionQueue::open_at(path.clone(), root.join("outbox")).unwrap();
        let mut message = QueuedMessage::new("上下文".into(), None);
        message.id = "durable-handoff".into();
        queue.enqueue_handoff("target", message.clone()).unwrap();
        queue.enqueue_handoff("target", message.clone()).unwrap();
        assert_eq!(queue.communication_evidence().len(), 1);
        queue.commit_next("target");
        drop(queue);
        let reopened = SessionActionQueue::open_at(path, root.join("outbox")).unwrap();
        reopened.enqueue_handoff("target", message).unwrap();
        assert!(
            reopened.peek_next("target").is_none(),
            "已消费交接不能在重启后重复入队"
        );
        assert!(reopened.handoff_enqueued("durable-handoff"));
        drop(reopened);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn handoff_does_not_stop_source_with_unsubmitted_text() {
        let queue = SessionActionQueue::new();
        queue
            .pending_submits
            .lock()
            .unwrap()
            .insert("source".into(), PendingSubmit { due: Instant::now(), message_id: "source-message".into(), pty_id: 0, submitted: false, awaiting_prompt_after_blocker: false });
        let mut stopped = false;
        assert!(queue
            .suspend_delivery_for_handoff("source", || {
                stopped = true;
                Ok(())
            })
            .is_err());
        assert!(!stopped);
        assert!(queue.pending_submits.lock().unwrap().contains_key("source"));
    }

    #[test]
    fn handoff_queue_persistence_failure_does_not_claim_enqueued() {
        let root =
            std::env::temp_dir().join(format!("crewvela-handoff-failure-{}", uuid::Uuid::new_v4()));
        let path = root.join("mailbox.json");
        let queue = SessionActionQueue::open_at(path.clone(), root.join("outbox")).unwrap();
        std::fs::create_dir(&path).unwrap(); // 原子重命名不能覆盖目录。
        let mut message = QueuedMessage::new("保留于交接草稿".into(), None);
        message.id = "failed-handoff".into();
        assert!(queue.enqueue_handoff("target", message).is_err());
        assert!(!queue.handoff_enqueued("failed-handoff"));
        assert!(queue.peek_next("target").is_none());
        drop(queue);
        std::fs::remove_dir_all(root).unwrap();
    }
}
