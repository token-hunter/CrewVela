//! Ordinary-session switching. Separate from collaboration ownership and scheduling.
use crate::control::{QueuedMessage, SessionActionQueue};
use crate::pty::PtyManager;
use crate::sessions::{
    NewRuntimeSession, PermissionMode, SessionKind, SessionManager, SessionRecord,
};
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use tauri::{Manager, State};

fn default_target_runtime() -> String {
    "codex".into()
}
pub(crate) const CLI_RUNTIMES: &[&str] = &[
    "claude_code",
    "codex",
    "kimi_code",
    "pi",
    "opencode",
    "zcode_cli",
];
pub(crate) const CHAT_RUNTIMES: &[&str] = &[
    "deepseek_harness",
    "claude_chat",
    "codex_chat",
    "kimi_chat",
    "opencode_chat",
    "pi_chat",
    "zcode",
];
pub(crate) fn source_runtime(source: &SessionRecord) -> &str {
    source
        .agent_runtime_kind
        .as_deref()
        .unwrap_or(if source.kind == SessionKind::Claude {
            "claude_code"
        } else {
            ""
        })
}
pub(crate) fn runtime(kind: &str) -> Result<crate::multi_agent::AgentRuntimeKind> {
    Ok(serde_json::from_value(serde_json::Value::String(
        kind.into(),
    ))?)
}
fn requires_plan(source: &SessionRecord) -> bool {
    source.permission_mode == Some(PermissionMode::Plan)
        || managed_records(&source.id).is_ok_and(|items| {
            items.iter().any(|item| {
                item.target_id.as_deref() == Some(&source.id)
                    && (item.source.permission_mode == Some(PermissionMode::Plan)
                        || matches!(
                            item.source_permission.as_deref(),
                            Some("plan" | "read-only")
                        ))
            })
        })
}
fn actual_permission<R: tauri::Runtime>(
    app: &tauri::AppHandle<R>,
    source: &SessionRecord,
) -> Result<Option<String>> {
    let actual = crate::session_switch_native::permission_mode(app, source)?;
    if requires_plan(source) || actual.as_deref() == Some("read-only") {
        Ok(Some("plan".into()))
    } else {
        Ok(actual)
    }
}
fn validate_pair(source: &SessionRecord, target: &str) -> Result<()> {
    eligible(source)?;
    let group = if CLI_RUNTIMES.contains(&source_runtime(source)) {
        CLI_RUNTIMES
    } else {
        CHAT_RUNTIMES
    };
    if !group.contains(&target) || target == source_runtime(source) {
        bail!("只能切换到同类的其它受支持 Agent");
    }
    runtime(target)?.ensure_available()?;
    if requires_plan(source) && !matches!(target, "codex" | "claude_code") {
        bail!("目标 Agent 无法保证来源 Plan 只读权限，拒绝切换");
    }
    Ok(())
}
static SWITCH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchRecord {
    pub id: String,
    pub source: SessionRecord,
    #[serde(default)]
    pub source_position: usize,
    pub source_pty: Option<u32>,
    pub target_id: Option<String>,
    pub thread_id: Option<String>,
    #[serde(default = "default_target_runtime")]
    pub target_runtime: String,
    #[serde(default)]
    pub logical_id: Option<String>,
    #[serde(default)]
    pub preserve_source: bool,
    #[serde(default)]
    pub recovery_dismissed: bool,
    #[serde(default)]
    pub source_history: Vec<crate::transcript::TranscriptMessage>,
    #[serde(default)]
    pub source_permission: Option<String>,
    pub state: String,
    pub history_mode: String,
    pub context: String,
    pub error: Option<String>,
}

fn open_at(path: &std::path::Path) -> Result<Connection> {
    let db = Connection::open(path)?;
    db.execute_batch("CREATE TABLE IF NOT EXISTS session_switches(id TEXT PRIMARY KEY, record_json TEXT NOT NULL);")?;
    Ok(db)
}
#[cfg(test)]
thread_local! { static TEST_SWITCH_DIR: std::cell::RefCell<Option<std::path::PathBuf>> = const { std::cell::RefCell::new(None) }; }
#[cfg(test)]
pub(crate) fn with_isolated_switches<T>(root: &std::path::Path, run: impl FnOnce() -> T) -> T {
    struct Restore(Option<std::path::PathBuf>);
    impl Drop for Restore {
        fn drop(&mut self) { TEST_SWITCH_DIR.with(|slot| *slot.borrow_mut() = self.0.take()); }
    }
    let previous = TEST_SWITCH_DIR.with(|slot| slot.replace(Some(root.into())));
    let _restore = Restore(previous);
    run()
}

fn switch_directory() -> Result<std::path::PathBuf> {
    #[cfg(test)]
    if let Some(path) = TEST_SWITCH_DIR.with(|slot| slot.borrow().clone()) {
        return Ok(path);
    }
    crate::providers::config_dir()
}
fn database() -> Result<Connection> {
    open_at(&switch_directory()?.join("session-switches.sqlite"))
}
fn existing_records() -> Result<Vec<SwitchRecord>> {
    let path = switch_directory()?.join("session-switches.sqlite");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let db = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    records(&db)
}
pub(crate) fn structured_zcode_cli(id: &str) -> bool {
    managed_records(id).is_ok_and(|records| {
        records.iter().any(|record| {
            record.target_id.as_deref() == Some(id) && record.target_runtime == "zcode_cli"
        })
    })
}
pub(crate) fn is_handoff_target(id: &str) -> Result<bool> {
    Ok(managed_records(id)?
        .iter()
        .any(|item| item.target_id.as_deref() == Some(id) && item.state != "cancelled"))
}
pub(crate) fn is_handoff_source(id: &str) -> Result<bool> {
    Ok(managed_records(id)?.iter().any(|item| item.source.id == id))
}
pub(crate) fn incoming_marker(id: &str) -> Result<Option<String>> {
    Ok(managed_records(id)?
        .iter()
        .rev()
        .find(|item| item.target_id.as_deref() == Some(id) && item.state != "cancelled")
        .map(|item| format!("[CrewVela handoff {}]", item.id)))
}
pub(crate) fn handoff_imported_thread(id: &str) -> Result<Option<String>> {
    Ok(managed_records(id)?
        .iter()
        .find(|item| item.target_id.as_deref() == Some(id) && item.state != "cancelled")
        .and_then(|item| item.thread_id.clone()))
}
fn marker_path(id: &str) -> Result<std::path::PathBuf> {
    uuid::Uuid::parse_str(id).context("接力会话 ID 无效")?;
    Ok(switch_directory()?.join("session-switch-markers").join(id))
}
fn mark_managed(id: &str) -> Result<()> {
    let path = marker_path(id)?;
    std::fs::create_dir_all(path.parent().context("缺少接力标记目录")?)?;
    std::fs::write(path, b"CrewVela managed session switch")?;
    Ok(())
}
fn managed_records(id: &str) -> Result<Vec<SwitchRecord>> {
    // Ordinary legacy sessions never open or depend on the new switch database.
    if uuid::Uuid::parse_str(id).is_err() || !marker_path(id)?.exists() {
        return Ok(Vec::new());
    }
    let items = existing_records()?;
    if !items
        .iter()
        .any(|item| item.source.id == id || item.target_id.as_deref() == Some(id))
    {
        bail!("接力标记存在但记录缺失，不能安全恢复执行");
    }
    Ok(items)
}
fn save(db: &Connection, item: &SwitchRecord) -> Result<()> {
    db.execute("INSERT INTO session_switches VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET record_json=excluded.record_json",
        params![item.id, serde_json::to_string(item)?])?;
    Ok(())
}
fn records(db: &Connection) -> Result<Vec<SwitchRecord>> {
    let mut statement = db.prepare("SELECT record_json FROM session_switches ORDER BY rowid")?;
    let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
    rows.map(|row| Ok(serde_json::from_str(&row?)?)).collect()
}
pub(crate) fn eligible(source: &SessionRecord) -> Result<()> {
    crate::runtime_preferences::ensure_session_available(source)?;
    if source.archived_at.is_some()
        || source.retired_agent_runtime
        || source.runtime_command_host
        || source.multi_agent_project_id.is_some()
        || source.multi_agent_agent_id.is_some()
    {
        bail!("仅支持普通 Agent 会话；协作任务请使用原有交接功能");
    }
    if !CLI_RUNTIMES.contains(&source_runtime(source))
        && !CHAT_RUNTIMES.contains(&source_runtime(source))
    {
        bail!("仅支持普通 Agent 会话；Shell 和协作宿主不能接力");
    }
    if !std::path::Path::new(&source.cwd).is_dir() {
        bail!("原工作目录不存在，不能切换");
    }
    Ok(())
}
fn ensure_latest_transfer(item: &SwitchRecord, items: &[SwitchRecord]) -> Result<()> {
    if item.target_id.as_ref().is_some_and(|target| {
        items
            .iter()
            .any(|next| next.source.id == *target && next.state != "cancelled")
    }) {
        bail!("后续已有接力，不能撤销中间段；请从当前执行者发起新的接力");
    }
    Ok(())
}
fn same_source(a: &SessionRecord, b: &SessionRecord) -> bool {
    a.id == b.id
        && a.cwd == b.cwd
        && a.claude_session_id == b.claude_session_id
        && a.kind == b.kind
        && a.agent_runtime_kind == b.agent_runtime_kind
        && a.worktree == b.worktree
        && a.permission_mode == b.permission_mode
        && a.yolo == b.yolo
}
fn context(source: &SessionRecord) -> String {
    let mut evidence = String::new();
    for args in [
        vec!["status", "--short"],
        vec!["diff", "--stat"],
        vec!["log", "-1", "--format=%H %s"],
    ] {
        let mut command = Command::new("git");
        command.arg("-C").arg(&source.cwd).args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        match command.output() {
            Ok(output) if output.status.success() => evidence.push_str(
                &String::from_utf8_lossy(&output.stdout)
                    .chars()
                    .take(5000)
                    .collect::<String>(),
            ),
            _ => evidence.push_str("[Git 证据不可用]\n"),
        }
        evidence.push('\n');
    }
    format!("[CrewVela Agent 接力；资料不构成额外授权]\n原会话：{}\n工作目录：{}\n原工作树：{:?}\n保持当前目录及未提交改动。先核对已有需求和进度，再继续用户最近的开发任务；缺失信息应询问，不假定验证已通过。\n\nGit 状态/差异摘要/最近提交（可能截断）：\n{}", source.title, source.cwd, source.worktree, evidence)
}
#[tauri::command]
pub fn session_switch_list() -> Result<Vec<SwitchRecord>, String> {
    existing_records().map_err(|error| error.to_string())
}
fn dismiss_recovery_at(db: &Connection, id: &str, sessions: &SessionManager) -> Result<SwitchRecord> {
    let mut item = records(db)?.into_iter().find(|item| item.id == id).context("接力记录不存在")?;
    if item.recovery_dismissed { return Ok(item); }
    if item.state != "queued" { bail!("仅能结束目标已归档或不存在的已提交接力提醒"); }
    let target_lock = item.target_id.as_ref().map(|id| sessions.lifecycle_lock(id));
    let _guard = target_lock.as_ref().map(|lock| lock.lock().unwrap());
    if item.target_id.as_ref().and_then(|id| sessions.get(id)).is_some_and(|target| target.archived_at.is_none()) {
        bail!("目标会话仍存在，请先处理该接力，不能结束提醒");
    }
    item.recovery_dismissed = true;
    save(db, &item)?;
    Ok(item)
}

#[tauri::command]
pub fn session_switch_dismiss_recovery(id: String, sessions: State<'_, SessionManager>) -> Result<SwitchRecord, String> {
    (|| -> Result<_> {
        let _lock = SWITCH_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        dismiss_recovery_at(&database()?, &id, &sessions)
    })().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn session_switch_history(
    session_id: String,
    app: tauri::AppHandle,
) -> Result<Vec<crate::transcript::TranscriptMessage>, String> {
    (|| -> Result<_> {
        let items = existing_records()?;
        let root = items
            .iter()
            .find(|item| item.source.id == session_id)
            .context("原接力历史不存在")?;
        let logical = root.logical_id.as_ref().unwrap_or(&root.source.id);
        let mut messages = Vec::new();
        for item in items.iter().filter(|item| {
            item.state != "cancelled"
                && item.logical_id.as_ref().unwrap_or(&item.source.id) == logical
        }) {
            messages.push(crate::transcript::TranscriptMessage {
                role: "system".into(),
                text: format!(
                    "执行者：{}；会话：{}",
                    source_runtime(&item.source),
                    item.source.title
                ),
                at: None,
            });
            let history = if item.source_history.is_empty() {
                crate::session_switch_native::history(
                    Some(&app),
                    &item.source,
                    &app.state::<SessionManager>(),
                )?
            } else {
                item.source_history.clone()
            };
            messages.extend(history);
        }
        if messages.is_empty() {
            let source = app
                .state::<SessionManager>()
                .get(&session_id)
                .unwrap_or_else(|| root.source.clone());
            messages = crate::session_switch_native::history(
                Some(&app),
                &source,
                &app.state::<SessionManager>(),
            )
            .or_else(|error| {
                if root.source_history.is_empty() {
                    Err(error)
                } else {
                    Ok(root.source_history.clone())
                }
            })?;
        }
        Ok(messages)
    })()
    .map_err(|error| error.to_string())
}
#[tauri::command]
pub fn session_switch_preview(
    source_id: String,
    source_position: Option<usize>,
    target_runtime: Option<String>,
    app: tauri::AppHandle,
    sessions: State<'_, SessionManager>,
) -> Result<SwitchRecord, String> {
    prepare_switch_preview(source_id, source_position, target_runtime, &app, &sessions)
}

fn prepare_switch_preview<R: tauri::Runtime>(
    source_id: String,
    source_position: Option<usize>,
    target_runtime: Option<String>,
    app: &tauri::AppHandle<R>,
    sessions: &SessionManager,
) -> Result<SwitchRecord, String> {
    (|| -> Result<_> {
        let _lock = SWITCH_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let db = database()?;
        let target_runtime = target_runtime.unwrap_or_else(default_target_runtime);
        if let Some(existing) = records(&db)?.into_iter().find(|item| {
            item.source.id == source_id
                && item.state != "cancelled"
                && item.target_runtime == target_runtime
        }) {
            mark_managed(&existing.source.id)?;
            return Ok(existing);
        }
        let source = sessions.get(&source_id).context("原会话不存在")?;
        validate_pair(&source, &target_runtime)?;
        crate::session_switch_permissions::preflight(&target_runtime)?;
        let source_permission = actual_permission(app, &source)?;
        if source_permission.as_deref() == Some("plan")
            && !matches!(target_runtime.as_str(), "codex" | "claude_code")
        {
            bail!("来源处于 Plan 模式；目标无法保证只读，拒绝切换");
        }
        if records(&db)?
            .iter()
            .any(|item| item.source.id == source_id && item.state != "cancelled")
        {
            bail!("来源已有接力，请先处理已有接力");
        }
        crate::multi_agent::resolve_runtime_executable(runtime(&target_runtime)?)
            .context("未检测到目标 Agent；请先安装并配置")?;
        let logical_id = records(&db)?
            .iter()
            .rev()
            .find(|item| item.target_id.as_deref() == Some(&source_id) && item.state != "cancelled")
            .map(|item| {
                item.logical_id
                    .clone()
                    .unwrap_or_else(|| item.source.id.clone())
            })
            .unwrap_or_else(|| source_id.clone());
        let mut preview_context = context(&source);
        let mut source_history =
            crate::session_switch_native::history(Some(app), &source, sessions).unwrap_or_else(
                |error| {
                    preview_context
                        .push_str(&format!("\n[来源历史暂不可读：{error}；交接资料不完整]\n"));
                    Vec::new()
                },
            );
        if let Some(pty) = source.pty_id {
            if let Ok(bytes) = app.state::<PtyManager>().transcript_tail(pty, 12000) {
                source_history.push(crate::transcript::TranscriptMessage {
                    role: "system".into(),
                    text: crate::collaboration_support::terminal_reference(&bytes),
                    at: None,
                });
            }
        }
        preview_context = bounded_history(&preview_context, &source_history);
        let item = SwitchRecord {
            id: uuid::Uuid::new_v4().to_string(),
            source_pty: source.pty_id,
            source_position: source_position.unwrap_or_else(|| {
                sessions
                    .list()
                    .iter()
                    .position(|item| item.id == source.id)
                    .unwrap_or(0)
            }),
            logical_id: Some(logical_id),
            target_runtime,
            source_history,
            preserve_source: true,
            recovery_dismissed: false,
            source_permission,
            context: preview_context,
            source,
            target_id: None,
            thread_id: None,
            state: "preview".into(),
            history_mode: "handoff".into(),
            error: None,
        };
        save(&db, &item)?;
        mark_managed(&item.source.id)?;
        Ok(item)
    })()
    .map_err(|error| error.to_string())
}
pub(crate) fn ensure_can_start(id: &str) -> Result<()> {
    check_runtime_start(id, false, None).map(|_| ())
}
pub(crate) fn check_runtime_start(id: &str, yolo: bool, switch_id: Option<&str>) -> Result<bool> {
    check_start_records(&managed_records(id)?, id, yolo, switch_id)
}
fn check_start_records(
    items: &[SwitchRecord],
    id: &str,
    yolo: bool,
    switch_id: Option<&str>,
) -> Result<bool> {
    let mut safe_yolo = yolo;
    for item in items {
        if item.source.id == id && !matches!(item.state.as_str(), "preview" | "cancelled") {
            bail!("来源 Agent 会话已进入 Agent 接力，请在切换窗口重试或显式恢复原会话");
        }
        if item.target_id.as_deref() == Some(id) {
            safe_yolo = false;
            if item.state != "queued"
                && !(switch_id == Some(&item.id)
                    && matches!(item.state.as_str(), "imported" | "launching"))
            {
                bail!("接手会话尚未完成接力或已取消，请在切换窗口操作");
            }
        }
    }
    Ok(safe_yolo)
}
pub(crate) fn protect_delete(id: &str, sessions: &SessionManager) -> Result<()> {
    for item in managed_records(id)? {
        if item.source.id == id
            && !matches!(item.state.as_str(), "preview" | "queued" | "cancelled")
        {
            bail!("该会话的接力尚未完成；请先在切换窗口恢复或完成接力");
        }
        if item.target_id.as_deref() == Some(id)
            && !matches!(item.state.as_str(), "queued" | "cancelled")
        {
            bail!("接手会话正在交接，不能删除");
        }
        if item.source.id == id
            && item
                .target_id
                .as_ref()
                .is_some_and(|target| sessions.get(target).is_some())
        {
            bail!("该记录保留接力历史及工作树所有权；请先删除关联的接手会话");
        }
    }
    Ok(())
}
pub(crate) fn protect_bulk_delete(sessions: &SessionManager) -> Result<()> {
    if !sessions
        .list()
        .iter()
        .any(|record| marker_path(&record.id).is_ok_and(|path| path.exists()))
    {
        return Ok(());
    }
    for item in existing_records()? {
        if !matches!(item.state.as_str(), "preview" | "cancelled")
            && (sessions.get(&item.source.id).is_some()
                || item
                    .target_id
                    .as_ref()
                    .is_some_and(|id| sessions.get(id).is_some()))
        {
            bail!("存在 Agent 接力关系，请先逐个删除接手会话或恢复原会话，再清空会话");
        }
    }
    Ok(())
}
pub(crate) fn check_approval(id: &str, policy: crate::sessions::ApprovalPolicy) -> Result<()> {
    if policy == crate::sessions::ApprovalPolicy::Allow
        && managed_records(id)?
            .iter()
            .any(|item| item.target_id.as_deref() == Some(id))
    {
        bail!("接力 Agent 使用保守审批，不能自动批准请求");
    }
    Ok(())
}
pub(crate) fn configure_target(command: &mut Command, record: &SessionRecord) -> Result<()> {
    let Some(item) = managed_records(&record.id)?
        .into_iter()
        .find(|item| item.target_id.as_deref() == Some(&record.id))
    else {
        return Ok(());
    };
    configure_target_record(command, record, &item)
}
fn configure_target_record(
    command: &mut Command,
    record: &SessionRecord,
    item: &SwitchRecord,
) -> Result<()> {
    if item.state == "cancelled" {
        bail!("接手会话已取消");
    }
    if record.cwd != item.source.cwd {
        bail!("接手工作目录已变化，拒绝启动");
    }
    if source_runtime(record) != item.target_runtime {
        bail!("接手运行时身份变化，拒绝启动");
    }
    if item.target_runtime != "codex" {
        if item.target_runtime == "claude_code" {
            command.args([
                "--permission-mode",
                if item.source.permission_mode == Some(PermissionMode::Plan)
                    || item.source_permission.as_deref() == Some("plan")
                {
                    "plan"
                } else {
                    "default"
                },
            ]);
        }
        if item.target_runtime == "pi" {
            let directory = switch_directory()?.join("runtime-extensions");
            std::fs::create_dir_all(&directory)?;
            let extension = directory.join("crewvela-pi-handoff-approval.mjs");
            std::fs::write(&extension, include_str!("pi-chat-bridge.mjs"))?;
            command.arg("-e").arg(extension);
        }
        if item.target_runtime == "opencode" {
            crate::session_switch_permissions::opencode(command, &record.id, true)?;
        }
        return Ok(());
    }
    if let Some(thread) = &item.thread_id {
        command.arg("resume").arg(thread);
    }
    command
        .arg("--sandbox")
        .arg(
            if item.source.permission_mode == Some(PermissionMode::Plan)
                || item.source_permission.as_deref() == Some("plan")
            {
                "read-only"
            } else {
                "workspace-write"
            },
        )
        .arg("--ask-for-approval")
        .arg("on-request");
    Ok(())
}
#[tauri::command]
pub async fn session_switch_confirm(
    id: String,
    allow_handoff: bool,
    app: tauri::AppHandle,
) -> Result<SwitchRecord, String> {
    tauri::async_runtime::spawn_blocking(move || {
        confirm(
            &id,
            allow_handoff,
            &app.state::<SessionManager>(),
            &app.state::<PtyManager>(),
            &app.state::<SessionActionQueue>(),
            Some(&app),
        )
    })
    .await
    .map_err(|error| error.to_string())?
}
fn confirm(
    id: &str,
    allow_handoff: bool,
    sessions: &SessionManager,
    ptys: &PtyManager,
    queue: &SessionActionQueue,
    app: Option<&tauri::AppHandle>,
) -> Result<SwitchRecord, String> {
    (|| -> Result<_> {
        let _lock = SWITCH_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let db = database()?;
        let mut item = records(&db)?
            .into_iter()
            .find(|item| item.id == id)
            .context("切换预览不存在")?;
        if item.state == "queued" {
            return Ok(item);
        }
        if item.state == "cancelled" {
            bail!("切换已取消，请重新预览");
        }
        let lifecycle = sessions.lifecycle_lock(&item.source.id);
        let _lifecycle = lifecycle.lock().unwrap();
        let result = execute(&db, &mut item, allow_handoff, &sessions, &ptys, &queue, app);
        if let Err(error) = result {
            item.error = Some(error.to_string());
            save(&db, &item)?;
        }
        Ok(item)
    })()
    .map_err(|error| error.to_string())
}
fn execute<R: tauri::Runtime>(
    db: &Connection,
    item: &mut SwitchRecord,
    allow_handoff: bool,
    sessions: &SessionManager,
    ptys: &PtyManager,
    queue: &SessionActionQueue,
    app: Option<&tauri::AppHandle<R>>,
) -> Result<()> {
    if item.state == "queued" {
        return Ok(());
    }
    let _target_runtime_lease =
        crate::runtime_preferences::acquire(runtime(&item.target_runtime)?)?;
    let source = sessions.get(&item.source.id).context("原会话不存在")?;
    let _source_runtime_lease = crate::runtime_preferences::session_runtime(&source)?
        .map(crate::runtime_preferences::acquire)
        .transpose()?;
    let mut eligibility_source = source.clone();
    if item.state != "preview" {
        eligibility_source.archived_at = None;
    }
    validate_pair(&eligibility_source, &item.target_runtime)?;
    crate::session_switch_permissions::preflight(&item.target_runtime)?;
    if let Some(app) = app {
        if item.source_permission.is_some()
            && item.source_permission != actual_permission(app, &source)?
        {
            bail!("来源实际权限模式已变化，请取消后重新准备交接");
        }
    }
    if !same_source(&source, &item.source) {
        bail!("原会话身份/目录/权限已变化，请恢复原会话后重新预览");
    }
    if matches!(item.state.as_str(), "preview" | "stopping") {
        if source.pty_id != item.source_pty {
            bail!("原进程已变化，请重新预览");
        }
        // PTY destruction removes its transcript buffer. Persist this evidence
        // before stopping, also retaining it if the app crashes during stop.
        if matches!(item.state.as_str(), "preview" | "stopping") {
            if let Some(pty) = source.pty_id {
                if let Ok(bytes) = ptys.transcript_tail(pty, 12000) {
                    item.source_history.retain(|message| {
                        !(message.role == "system" && message.text.starts_with("[只保留最近最多"))
                    });
                    item.source_history
                        .push(crate::transcript::TranscriptMessage {
                            role: "system".into(),
                            text: crate::collaboration_support::terminal_reference(&bytes),
                            at: None,
                        });
                    save(db, item)?;
                }
            }
        }
        stop_before_transfer(db, item, || {
            queue.suspend_delivery_for_handoff(&source.id, || {
                if let Some(pty) = source.pty_id {
                    ptys.stop_for_handoff(pty)?;
                }
                if crate::session_switch_native::is_native(&source) {
                    crate::session_switch_native::stop(
                        app.context("缺少原生应用上下文")?,
                        &source,
                    )?;
                } else {
                    sessions.mark_stopped(&source.id)?;
                }
                Ok(())
            })
        })?;
    } else if source.pty_id.is_some() {
        bail!("原 Agent 已恢复运行，请先停止原执行");
    }
    if !item.preserve_source {
        sessions.archive(&source.id)?;
    }
    if item.target_id.is_none() {
        item.context = context(&source);
    }
    item.error = None;
    if item.target_id.is_none() {
        let previous_history = std::mem::take(&mut item.source_history);
        item.source_history = crate::session_switch_native::history(app, &source, sessions)
            .unwrap_or_else(|error| {
                item.context.push_str(&format!(
                    "\n[来源最终历史读取不可用：{error}；使用先前快照与终端线索，交接资料不完整，必须核实用户需求]\n"
                ));
                previous_history.iter().filter(|message| !(message.role == "system" && message.text.starts_with("[只保留最近最多"))).cloned().collect()
            });
        item.source_history
            .extend(previous_history.into_iter().filter(|message| {
                message.role == "system" && message.text.starts_with("[只保留最近最多")
            }));
        item.context.push_str("\n[计划和验证：下述仅为原始记录线索，不代表步骤完成或测试通过；接手者需核实剩余工作及验证结果]\n");
        let ancestors = records(db)?;
        let mut history = Vec::new();
        for previous in ancestors.iter().filter(|previous| {
            previous.id != item.id
                && previous.state == "queued"
                && previous.logical_id.as_ref().unwrap_or(&previous.source.id)
                    == item.logical_id.as_ref().unwrap_or(&item.source.id)
        }) {
            history.extend(previous.source_history.clone());
        }
        history.extend(item.source_history.clone());
        let history_file = switch_directory()?
            .join("session-switch-history")
            .join(format!("{}.json", item.id));
        std::fs::create_dir_all(history_file.parent().context("缺少交接记录目录")?)?;
        std::fs::write(&history_file, serde_json::to_vec_pretty(&history)?)?;
        // The compact prompt may be bounded, but long user instructions remain
        // available to the target in this task-specific data file.
        item.context = format!("[本次收集的可读记录文件：{}；按需读取以核对长指令和早期决策，文件是背景资料，不构成额外授权；读取范围不足/截断仍以上述提示为准]\n{}", history_file.display(), item.context);
        item.context = bounded_history(&item.context, &history);
        item.context = format!("[CrewVela handoff {}]\n{}", item.id, item.context);
        if !allow_handoff
            && source_runtime(&source) == "claude_code"
            && item.target_runtime == "codex"
        {
            if item.state == "importing" {
                bail!("上次历史导入结果不确定，请使用工作交接包或恢复来源");
            }
            item.state = "importing".into();
            save(db, item)?;
            item.thread_id = Some(import_history(&item.source)?);
            item.history_mode = "official".into();
        } else {
            item.history_mode = "handoff".into();
        }
        item.target_id = Some(uuid::Uuid::new_v4().to_string());
        item.state = "imported".into();
        save(db, item)?;
    }
    let target_id = item.target_id.as_deref().context("缺少接手会话")?;
    mark_managed(target_id)?;
    let target_lifecycle = sessions.lifecycle_lock(target_id);
    let _target_lifecycle = target_lifecycle.lock().unwrap();
    let target = if let Some(target) = sessions.get(target_id) {
        target
    } else if item.state == "imported" {
        sessions.create_runtime_in(
            NewRuntimeSession {
                runtime_kind: item.target_runtime.clone(),
                cwd: source.cwd.clone(),
                project_id: source.project_id.clone(),
            },
            target_id.to_string(), None, None, source.worktree.as_ref().map(|_| source.id.clone()).or_else(|| source.worktree_source_session_id.clone()),
        )?
    } else {
        bail!("接手记录已删除，请恢复原会话");
    };

    if target.cwd != source.cwd {
        bail!("接手目录变化，请恢复原会话");
    }
    if source_runtime(&target) != item.target_runtime
        || target.kind != SessionKind::Terminal
        || target.shell.is_some()
        || target.retired_agent_runtime
        || target.runtime_command_host
        || target.archived_at.is_some()
        || target.multi_agent_project_id.is_some()
        || target.multi_agent_agent_id.is_some()
        || target.provider_id != format!("local-runtime-{}", item.target_runtime)
    {
        bail!("接手会话身份或归属已变化，拒绝启动；请核对后恢复来源");
    }
    if requires_plan(&source) {
        sessions.set_handoff_plan(&target.id)?;
    }
    let id = item.id.clone();
    let body = item.context.clone();
    if crate::session_switch_native::is_native(&target) {
        let app = app.context("缺少原生应用上下文")?;
        if item.state == "sending" {
            // ACP/Pi local history is written before their asynchronous transport.
            // Only ZCode's native admission evidence can settle an interrupted send.
            if matches!(item.target_runtime.as_str(), "zcode" | "zcode_cli")
                && crate::session_switch_native::has_submission(app, &target, &id)?
            {
                item.state = "queued".into();
                save(db, item)?;
                return Ok(());
            }
            bail!("原生输入投递结果不确定；请核对接手历史后恢复来源，不能自动重复提交");
        }
        item.state = "launching".into();
        save(db, item)?;
        crate::session_switch_native::start(app, &target)?;
        item.state = "sending".into();
        save(db, item)?;
        if let Err(error) = crate::session_switch_native::send(app, &target, &id, &body) {
            if !error.to_string().contains("UNCERTAIN")
                && !crate::session_switch_native::has_submission(app, &target, &id)?
            {
                item.state = "launching".into();
                save(db, item)?;
            }
            return Err(error);
        }
        item.state = "queued".into();
        save(db, item)?;
    } else {
        finish_transfer(
            db,
            item,
            || {
                crate::open_runtime_session_for_switch(&target, ptys, sessions, &id)
                    .map(|_| ())
                    .map_err(|error| anyhow!(error))
            },
            || {
                let mut message = QueuedMessage::new(body, None);
                message.id = id.clone();
                queue.enqueue_handoff(&target.id, message)
            },
        )?;
    }
    sessions.rename(&target.id, &source.title)?;
    Ok(())
}
fn stop_before_transfer(
    db: &Connection,
    item: &mut SwitchRecord,
    stop: impl FnOnce() -> Result<()>,
) -> Result<()> {
    item.state = "stopping".into();
    save(db, item)?;
    stop()?;
    item.state = "stopped".into();
    save(db, item)
}
fn finish_transfer(
    db: &Connection,
    item: &mut SwitchRecord,
    start: impl FnOnce() -> Result<()>,
    queue: impl FnOnce() -> Result<()>,
) -> Result<()> {
    if item.state == "queued" {
        return Ok(());
    }
    if !matches!(item.state.as_str(), "imported" | "launching") {
        bail!("来源未停止或历史未准备，不能启动接手者");
    }
    item.state = "launching".into();
    save(db, item)?;
    start()?;
    queue()?;
    item.state = "queued".into();
    save(db, item)
}
#[tauri::command]
pub fn session_switch_restore(
    id: String,
    sessions: State<'_, SessionManager>,
    ptys: State<'_, PtyManager>,
    queue: State<'_, SessionActionQueue>,
    app: tauri::AppHandle,
) -> Result<SwitchRecord, String> {
    restore_switch(&id, &sessions, &ptys, &queue, &app)
}
fn restore_switch<R: tauri::Runtime>(
    id: &str,
    sessions: &SessionManager,
    ptys: &PtyManager,
    queue: &SessionActionQueue,
    app: &tauri::AppHandle<R>,
) -> Result<SwitchRecord, String> {
    (|| -> Result<_> {
        let _lock = SWITCH_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
        let db = database()?;
        let mut item = records(&db)?
            .into_iter()
            .find(|item| item.id == id)
            .context("切换记录不存在")?;
        ensure_latest_transfer(&item, &records(&db)?)?;
        let lifecycle = sessions.lifecycle_lock(&item.source.id);
        let _lifecycle = lifecycle.lock().unwrap();
        let target_lifecycle = item
            .target_id
            .as_ref()
            .map(|id| sessions.lifecycle_lock(id));
        let _target_lifecycle = target_lifecycle.as_ref().map(|lock| lock.lock().unwrap());
        if let Some(target) = item.target_id.as_ref().and_then(|id| sessions.get(id)) {
            queue.suspend_delivery_for_handoff(&target.id, || {
                if let Some(pty) = target.pty_id {
                    ptys.stop_for_handoff(pty)?;
                }
                if crate::session_switch_native::is_native(&target) {
                    crate::session_switch_native::stop(app, &target)?;
                } else {
                    sessions.mark_stopped(&target.id)?;
                }
                sessions.archive(&target.id)?;
                Ok(())
            })?;
        }
        item.state = "cancelled".into();
        item.error = None;
        save(&db, &item)?;
        sessions.restore(&item.source.id)?;
        Ok(item)
    })()
    .map_err(|error| error.to_string())
}

struct ImportProcess {
    _runtime_lease: crate::runtime_preferences::RuntimeLease,
    child: std::process::Child,
    input: std::process::ChildStdin,
    lines: std::sync::mpsc::Receiver<String>,
    pending: Vec<serde_json::Value>,
    deadline: std::time::Instant,
}
fn bounded_history(base: &str, messages: &[crate::transcript::TranscriptMessage]) -> String {
    let mut result = base.chars().take(10000).collect::<String>();
    let latest_user = messages.iter().rposition(|message| message.role == "user");
    let mut remaining = 30000usize.saturating_sub(result.chars().count());
    let mut selected = Vec::new();
    // Give the latest instruction priority over terminal redraws and older context.
    if let Some(index) = latest_user {
        let text = &messages[index].text;
        let count = text.chars().count();
        if count + 32 <= remaining {
            let line = format!("\n[最近用户指令，原文]\n{text}\n");
            remaining -= line.chars().count();
            selected.push((index, line));
        } else {
            result.push_str("\n[最近用户指令超出交接容量，资料不完整：必须回看原历史核对全文，不能假定已完整交接]\n");
            let line = format!(
                "用户指令尾部：{}\n",
                text.chars()
                    .rev()
                    .take(remaining.saturating_sub(64))
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            );
            remaining = 0;
            selected.push((index, line));
        }
    }
    for (index, message) in messages.iter().enumerate().rev() {
        if Some(index) == latest_user || remaining < 64 {
            continue;
        }
        let max = if message.role == "system" { 3500 } else { 5000 };
        let line = format!(
            "{}: {}{}\n",
            message.role,
            message.text.chars().take(max).collect::<String>(),
            if message.text.chars().count() > max {
                " [此条截断]"
            } else {
                ""
            }
        );
        let bounded = line.chars().take(remaining).collect::<String>();
        remaining -= bounded.chars().count();
        selected.push((index, bounded));
    }
    selected.sort_by_key(|(index, _)| *index);
    for (_, line) in selected {
        result.push_str(&line);
    }
    result.push_str("\n[历史按总容量限制保留，可能截断；原始记录可回看，验证结论需核实]\n");
    result
}

impl Drop for ImportProcess {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let _ = Command::new("taskkill")
                .args(["/PID", &self.child.id().to_string(), "/T", "/F"])
                .creation_flags(0x08000000)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl ImportProcess {
    fn start(cwd: &str) -> Result<Self> {
        let runtime_lease =
            crate::runtime_preferences::acquire(crate::multi_agent::AgentRuntimeKind::Codex)?;
        use std::io::BufRead;
        let path = crate::multi_agent::resolve_runtime_executable(
            crate::multi_agent::AgentRuntimeKind::Codex,
        )
        .context("未检测到 Codex CLI")?;
        let mut command = crate::multi_agent::command_for_path(&path);
        command
            .arg("app-server")
            .current_dir(cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().context("启动 Codex 历史导入服务失败")?;
        let input = child.stdin.take().context("缺少导入服务输入")?;
        let output = child.stdout.take().context("缺少导入服务输出")?;
        let (sender, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(output).lines() {
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
        Ok(Self {
            _runtime_lease: runtime_lease,
            child,
            input,
            lines,
            pending: Vec::new(),
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(120),
        })
    }
    fn send(&mut self, value: serde_json::Value) -> Result<()> {
        use std::io::Write;
        writeln!(self.input, "{}", value)?;
        self.input.flush()?;
        Ok(())
    }
    fn receive(&mut self) -> Result<serde_json::Value> {
        loop {
            let remaining = self
                .deadline
                .checked_duration_since(std::time::Instant::now())
                .context("历史导入超过 120 秒；原会话保持停止，可重试或恢复")?;
            let line = self
                .lines
                .recv_timeout(remaining)
                .context("Codex 历史导入超时或服务已退出")?;
            if let Ok(value) = serde_json::from_str(&line) {
                return Ok(value);
            }
        }
    }
    fn response(&mut self, id: u64) -> Result<serde_json::Value> {
        loop {
            let value = self.receive()?;
            if value.get("id").and_then(serde_json::Value::as_u64) == Some(id) {
                if let Some(error) = value.get("error") {
                    bail!(
                        "Codex 导入 API 不可用：{}",
                        error
                            .get("message")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("请求失败")
                    );
                }
                return value
                    .get("result")
                    .cloned()
                    .context("导入服务响应缺少 result");
            }
            self.pending.push(value);
        }
    }
    fn completed(&mut self, import_id: &str) -> Result<serde_json::Value> {
        loop {
            if let Some(index) = self
                .pending
                .iter()
                .position(|value| completion_matches(value, import_id))
            {
                return Ok(self.pending.remove(index));
            }
            let value = self.receive()?;
            if completion_matches(&value, import_id) {
                return Ok(value);
            }
        }
    }
}
fn completion_matches(value: &serde_json::Value, id: &str) -> bool {
    value.get("method").and_then(serde_json::Value::as_str)
        == Some("externalAgentConfig/import/completed")
        && value
            .pointer("/params/importId")
            .and_then(serde_json::Value::as_str)
            == Some(id)
}
fn same_path(a: &str, b: &str) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}
fn imported_thread(value: &serde_json::Value, source_path: &str, cwd: &str) -> Result<String> {
    let results = value
        .pointer("/params/itemTypeResults")
        .and_then(serde_json::Value::as_array)
        .context("导入完成通知缺少结果")?;
    let result = results
        .iter()
        .find(|item| item.get("itemType").and_then(serde_json::Value::as_str) == Some("SESSIONS"))
        .context("没有会话导入结果")?;
    if let Some(failures) = result.get("failures").and_then(serde_json::Value::as_array) {
        if !failures.is_empty() {
            bail!(
                "会话历史导入失败：{}",
                failures[0]
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("请更新 Codex 后重试")
            );
        }
    }
    let successes = result
        .get("successes")
        .and_then(serde_json::Value::as_array)
        .context("缺少导入成功记录")?;
    if successes.len() != 1 {
        bail!("导入成功记录数量不匹配；未启动接手 Agent");
    }
    let success = &successes[0];
    if !same_path(
        success
            .get("source")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        source_path,
    ) || !same_path(
        success
            .get("cwd")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
        cwd,
    ) {
        bail!("导入来源或工作目录不匹配；未启动接手 Agent");
    }
    let target = success
        .get("target")
        .and_then(serde_json::Value::as_str)
        .context("成功记录缺少 Codex threadId")?;
    uuid::Uuid::parse_str(target).context("导入 threadId 无效")?;
    Ok(target.to_owned())
}
fn import_history(source: &SessionRecord) -> Result<String> {
    let path = crate::usage::transcript_path(source)
        .filter(|path| path.is_file())
        .context("原 Claude 对话文件尚不存在或无法定位；可显式选择工作交接包继续")?;
    let path = path.canonicalize()?.display().to_string();
    let mut process = ImportProcess::start(&source.cwd)?;
    process.send(serde_json::json!({"id":1,"method":"initialize","params":{
        "clientInfo":{"name":"crewvela_session_switch","version":"0.1.0"},
        "capabilities":{"experimentalApi":true}}}))?;
    process.response(1)?;
    process.send(serde_json::json!({"method":"initialized","params":{}}))?;
    process.send(serde_json::json!({"id":2,"method":"externalAgentConfig/import","params":{"migrationItems":[{
        "itemType":"SESSIONS","description":"CrewVela Claude Code session transfer","cwd":null,
        "details":{"plugins":[],"sessions":[{"path":path,"cwd":source.cwd,"title":null}],
        "mcpServers":[],"hooks":[],"subagents":[],"commands":[]}}]}}))?;
    let response = process.response(2)?;
    let import_id = response
        .get("importId")
        .and_then(serde_json::Value::as_str)
        .filter(|id| !id.is_empty())
        .context("缺少 importId；未启动接手 Agent")?;
    imported_thread(&process.completed(import_id)?, &path, &source.cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sessions::NewSession;
    use std::cell::RefCell;

    struct Fixture {
        root: std::path::PathBuf,
        sessions: SessionManager,
        db: Connection,
    }
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("crewvela-switch-test-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let sessions =
                SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
            let db = open_at(&root.join("session-switches.sqlite")).unwrap();
            Self { root, sessions, db }
        }
        fn record(&self) -> SwitchRecord {
            let source = self
                .sessions
                .create(NewSession {
                    provider_id: "fixture".into(),
                    provider_name: "Fixture Claude".into(),
                    cwd: self.root.display().to_string(),
                    yolo: false,
                    auto_mode: false,
                    project_id: None,
                })
                .unwrap();
            SwitchRecord {
                id: uuid::Uuid::new_v4().to_string(),
                source_position: 0,
                preserve_source: false,
                recovery_dismissed: false,
                source_pty: None,
                source,
                target_id: None,
                thread_id: None,
                target_runtime: "codex".into(),
                logical_id: None,
                source_history: Vec::new(),
                source_permission: None,
                state: "preview".into(),
                history_mode: "official".into(),
                context: "fixture".into(),
                error: None,
            }
        }
    }
    #[test]
    fn dismiss_recovery_persists_without_restoring_source_or_removing_history() {
        let fixture = Fixture::new();
        let mut item = fixture.record();
        fixture.sessions.archive(&item.source.id).unwrap();
        item.state = "queued".into();
        item.target_id = Some("removed-target".into());
        save(&fixture.db, &item).unwrap();
        let before = serde_json::to_value(fixture.sessions.get(&item.source.id)).unwrap();
        let updated = dismiss_recovery_at(&fixture.db, &item.id, &fixture.sessions).unwrap();
        assert!(updated.recovery_dismissed);
        assert_eq!(updated.state, "queued");
        assert_eq!(updated.context, item.context);
        assert_eq!(serde_json::to_value(fixture.sessions.get(&item.source.id)).unwrap(), before);
        let reopened = open_at(&fixture.root.join("session-switches.sqlite")).unwrap();
        assert!(records(&reopened).unwrap()[0].recovery_dismissed);
        assert!(dismiss_recovery_at(&fixture.db, &item.id, &fixture.sessions).unwrap().recovery_dismissed);
        let mut legacy = serde_json::to_value(&item).unwrap();
        legacy.as_object_mut().unwrap().remove("recoveryDismissed");
        assert!(!serde_json::from_value::<SwitchRecord>(legacy).unwrap().recovery_dismissed);
    }

    #[test]
    fn dismiss_recovery_rejects_unfinished_states_and_live_targets_but_accepts_archived_target() {
        let fixture = Fixture::new();
        let mut item = fixture.record();
        for state in ["preview", "stopping", "launching", "sending", "cancelled"] {
            item.state = state.into(); save(&fixture.db, &item).unwrap();
            assert!(dismiss_recovery_at(&fixture.db, &item.id, &fixture.sessions).is_err());
            assert!(!records(&fixture.db).unwrap()[0].recovery_dismissed);
        }
        let target = fixture.record().source;
        item.state = "queued".into(); item.target_id = Some(target.id.clone());
        save(&fixture.db, &item).unwrap();
        assert!(dismiss_recovery_at(&fixture.db, &item.id, &fixture.sessions).is_err());
        fixture.sessions.archive(&target.id).unwrap();
        assert!(dismiss_recovery_at(&fixture.db, &item.id, &fixture.sessions).unwrap().recovery_dismissed);
    }

    #[test]
    fn dismiss_recovery_failed_save_does_not_end_the_persisted_reminder() {
        let fixture = Fixture::new();
        let mut item = fixture.record(); item.state = "queued".into();
        save(&fixture.db, &item).unwrap();
        let readonly = Connection::open_with_flags(fixture.root.join("session-switches.sqlite"), rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
        assert!(dismiss_recovery_at(&readonly, &item.id, &fixture.sessions).is_err());
        assert!(!records(&fixture.db).unwrap()[0].recovery_dismissed);
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[cfg(windows)]
    #[test]
    fn lifecycle_switch_never_archives_or_starts_target_after_refused_exited_root_cleanup() {
        let fixture = Fixture::new();
        let _directory = isolate_switches(&fixture.root);
        let mut item = fixture.record();
        let ptys = PtyManager::new();
        let (pty, root_pid, _) = crate::tests::lifecycle_node_pty(&ptys, &item.source.id, true);
        fixture.sessions.attach_runtime(&item.source.id, pty).unwrap();
        item.source_pty = Some(pty);
        let queue = SessionActionQueue::new();
        queue.enqueue_message(&item.source.id, crate::control::QueuedMessage::new("keep until stop verified".into(), None));
        let refusal = crate::acp::TestStopFailureGuard::new(root_pid);
        let result = execute(&fixture.db, &mut item, true, &fixture.sessions, &ptys, &queue, None::<&tauri::AppHandle<tauri::test::MockRuntime>>);
        assert!(result.is_err());
        assert_eq!(item.state, "stopping", "failure must actually reach checked stopping: {:?}", result.err());
        assert!(item.target_id.is_none());
        let source = fixture.sessions.get(&item.source.id).unwrap();
        assert_eq!(source.pty_id, Some(pty)); assert!(source.archived_at.is_none());
        assert!(queue.peek_next(&source.id).is_some());
        drop(refusal);
        stop_before_transfer(&fixture.db, &mut item, || {
            queue.suspend_delivery_for_handoff(&source.id, || {
                ptys.stop_for_handoff(pty)?; fixture.sessions.mark_stopped(&source.id)?; Ok(())
            })
        }).unwrap();
        assert_eq!(item.state, "stopped");
        assert!(fixture.sessions.get(&source.id).unwrap().pty_id.is_none());
    }

    #[test]
    fn runtime_preferences_disabled_target_rejects_switch_before_stopping_or_archiving_source() {
        crate::runtime_preferences::with_disabled_for_test(
            crate::multi_agent::AgentRuntimeKind::Codex,
            || {
                let fixture = Fixture::new();
                let mut item = fixture.record();
                let before = fixture.sessions.get(&item.source.id).unwrap();
                let ptys = PtyManager::new();
                let queue = SessionActionQueue::new();
                let error = execute(
                    &fixture.db,
                    &mut item,
                    true,
                    &fixture.sessions,
                    &ptys,
                    &queue,
                    None::<&tauri::AppHandle<tauri::test::MockRuntime>>,
                )
                .unwrap_err();
                assert!(error.to_string().contains("已全局禁用"));
                let after = fixture.sessions.get(&before.id).unwrap();
                assert_eq!(after.status, before.status);
                assert_eq!(after.pty_id, before.pty_id);
                assert_eq!(after.archived_at, before.archived_at);
                assert_eq!(item.state, "preview");
                assert!(item.target_id.is_none());
            },
        );
    }

    #[test]
    fn auto_wakeup_waits_for_lifecycle_and_reloads_its_stale_candidate() {
        use std::sync::mpsc;
        use std::time::Duration;

        let fixture = Fixture::new();
        let mut stale = fixture.record().source;
        // Both old and new implementations fail without launching an external Agent;
        // only the guarded implementation waits and observes the completed deletion.
        stale.retired_agent_runtime = true;
        let lifecycle = fixture.sessions.lifecycle_lock(&stale.id);
        let guard = lifecycle.lock().unwrap();
        let sessions = fixture.sessions.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let id = stale.id.clone();
        let worker = std::thread::spawn(move || {
            entered_tx.send(()).unwrap();
            let result =
                crate::open_direct_collaboration_session(&stale, &PtyManager::new(), &sessions, None);
            result_tx.send(result).unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let before_release = result_rx.recv_timeout(Duration::from_millis(100));
        fixture.sessions.delete(&id).unwrap();
        drop(guard);
        let result = result_rx.recv_timeout(Duration::from_secs(5));
        worker.join().unwrap();
        assert!(
            matches!(before_release, Err(mpsc::RecvTimeoutError::Timeout)),
            "auto-wakeup bypassed the active lifecycle operation"
        );
        assert_eq!(result.unwrap().unwrap_err(), "会话已删除");
    }

    #[test]
    fn auto_wakeup_rejects_runtime_retired_while_waiting_for_lifecycle() {
        use std::sync::mpsc;
        use std::time::Duration;

        let fixture = Fixture::new();
        let source = fixture
            .sessions
            .create_agent(crate::sessions::NewAgentSession {
                avatar_id: None,
                project_id: "project".into(),
                agent_id: "agent".into(),
                runtime_kind: "codex".into(),
                agent_name: "fixture".into(),
                cwd: fixture.root.display().to_string(),
            })
            .unwrap();
        let mut stale = source.clone();
        // An obsolete disabled-runtime snapshot makes regression failure safe:
        // even without the lock, the test cannot spawn an installed CLI.
        stale.agent_runtime_kind = Some("oh_my_pi".into());
        let lifecycle = fixture.sessions.lifecycle_lock(&source.id);
        let guard = lifecycle.lock().unwrap();
        let sessions = fixture.sessions.clone();
        let (entered_tx, entered_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            entered_tx.send(()).unwrap();
            result_tx
                .send(crate::open_direct_collaboration_session(
                    &stale,
                    &PtyManager::new(),
                    &sessions,
                    None,
                ))
                .unwrap();
        });
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let before_release = result_rx.recv_timeout(Duration::from_millis(100));
        fixture
            .sessions
            .retire_agent_runtime_sessions("project", "agent")
            .unwrap();
        drop(guard);
        let result = result_rx.recv_timeout(Duration::from_secs(5));
        worker.join().unwrap();
        assert!(matches!(
            before_release,
            Err(mpsc::RecvTimeoutError::Timeout)
        ));
        assert_eq!(
            result.unwrap().unwrap_err(),
            "该会话为 Runtime 切换前的只读历史，请选择成员当前会话"
        );
        let current = fixture.sessions.get(&source.id).unwrap();
        assert!(current.retired_agent_runtime && current.archived_at.is_some());
        assert!(current.pty_id.is_none());
    }

    #[test]
    fn ordinary_claude_stopped_supported_shell_collaboration_and_retired_rejected() {
        let fixture = Fixture::new();
        let record = fixture.record();
        let source = record.source;
        assert!(eligible(&source).is_ok());
        for variant in 0..5 {
            let mut source = source.clone();
            match variant {
                0 => source.multi_agent_project_id = Some("project".into()),
                1 => source.multi_agent_agent_id = Some("member".into()),
                2 => source.retired_agent_runtime = true,
                3 => source.runtime_command_host = true,
                _ => {
                    source.kind = SessionKind::Terminal;
                    source.shell = Some(crate::sessions::TerminalShell::Powershell);
                }
            }
            assert!(eligible(&source).is_err());
        }
    }
    #[test]
    fn stop_failure_persists_and_prevents_start_then_success_orders_stop_start_queue_and_is_idempotent(
    ) {
        let fixture = Fixture::new();
        let mut item = fixture.record();
        let order = RefCell::new(Vec::new());
        assert!(
            stop_before_transfer(&fixture.db, &mut item, || bail!("fixture stop failed")).is_err()
        );
        assert_eq!(records(&fixture.db).unwrap()[0].state, "stopping");
        assert!(finish_transfer(
            &fixture.db,
            &mut item,
            || {
                order.borrow_mut().push("start");
                Ok(())
            },
            || Ok(())
        )
        .is_err());
        assert!(order.borrow().is_empty());
        stop_before_transfer(&fixture.db, &mut item, || {
            order.borrow_mut().push("stop");
            Ok(())
        })
        .unwrap();
        assert_eq!(records(&fixture.db).unwrap()[0].state, "stopped");
        item.state = "imported".into();
        finish_transfer(
            &fixture.db,
            &mut item,
            || {
                order.borrow_mut().push("start");
                Ok(())
            },
            || {
                order.borrow_mut().push("queue");
                Ok(())
            },
        )
        .unwrap();
        finish_transfer(
            &fixture.db,
            &mut item,
            || panic!("must not start twice"),
            || panic!("must not queue twice"),
        )
        .unwrap();
        assert_eq!(*order.borrow(), vec!["stop", "start", "queue"]);
    }
    #[test]
    fn launch_or_queue_failure_is_durable_and_retry_preserves_identity() {
        let fixture = Fixture::new();
        let mut item = fixture.record();
        item.state = "imported".into();
        item.target_id = Some(uuid::Uuid::new_v4().to_string());
        item.thread_id = Some(uuid::Uuid::new_v4().to_string());
        assert!(finish_transfer(
            &fixture.db,
            &mut item,
            || bail!("launch failed"),
            || panic!("must not queue")
        )
        .is_err());
        let mut restored = records(&fixture.db).unwrap().remove(0);
        assert_eq!(restored.state, "launching");
        assert_eq!(restored.target_id, item.target_id);
        assert!(finish_transfer(
            &fixture.db,
            &mut restored,
            || Ok(()),
            || bail!("queue persistence failed")
        )
        .is_err());
        assert_eq!(records(&fixture.db).unwrap()[0].state, "launching");
        finish_transfer(&fixture.db, &mut restored, || Ok(()), || Ok(())).unwrap();
        assert_eq!(records(&fixture.db).unwrap()[0].state, "queued");
        assert_eq!(restored.thread_id, item.thread_id);
    }
    #[test]
    fn cancelled_attempt_survives_new_preview_and_target_never_gains_yolo_or_parallel_resume() {
        let fixture = Fixture::new();
        let mut old = fixture.record();
        old.state = "cancelled".into();
        let target = uuid::Uuid::new_v4().to_string();
        old.target_id = Some(target.clone());
        save(&fixture.db, &old).unwrap();
        let mut next = old.clone();
        next.id = uuid::Uuid::new_v4().to_string();
        next.state = "preview".into();
        next.target_id = None;
        save(&fixture.db, &next).unwrap();
        let items = records(&fixture.db).unwrap();
        assert_eq!(items.len(), 2);
        assert!(check_start_records(&items, &target, true, None).is_err());
        next.state = "imported".into();
        next.target_id = Some(uuid::Uuid::new_v4().to_string());
        save(&fixture.db, &next).unwrap();
        assert!(check_start_records(
            &[next.clone()],
            next.target_id.as_deref().unwrap(),
            true,
            None
        )
        .is_err());
        assert_eq!(
            check_start_records(
                &[next.clone()],
                next.target_id.as_deref().unwrap(),
                true,
                Some(&next.id)
            )
            .unwrap(),
            false
        );
        next.state = "queued".into();
        assert_eq!(
            check_start_records(
                &[next.clone()],
                next.target_id.as_deref().unwrap(),
                true,
                None
            )
            .unwrap(),
            false
        );
        assert!(check_start_records(&[next.clone()], &next.source.id, false, None).is_err());
        assert!(check_start_records(&[], "unrelated-session", true, None).unwrap());
    }
    #[test]
    fn reserved_target_is_idempotent_and_does_not_own_source_worktree() {
        let fixture = Fixture::new();
        let mut source = fixture.record().source;
        source.worktree = Some(crate::worktree::SessionWorktree {
            repo_root: fixture.root.display().to_string(),
            path: fixture.root.display().to_string(),
            branch: "fixture-branch".into(), binding: None,
        });
        let id = uuid::Uuid::new_v4().to_string();
        let create = || {
            fixture
                .sessions
                .create_runtime_with_id(
                    NewRuntimeSession {
                        runtime_kind: "codex".into(),
                        cwd: source.cwd.clone(),
                        project_id: source.project_id.clone(),
                    },
                    id.clone(),
                )
                .unwrap()
        };
        let target = create();
        assert_eq!(create().id, target.id);
        assert_eq!(target.cwd, source.cwd);
        assert!(target.worktree.is_none());
        assert!(!target.yolo);
        assert_eq!(
            fixture
                .sessions
                .list()
                .iter()
                .filter(|record| record.id == id)
                .count(),
            1
        );
    }
    #[test]
    fn native_resume_and_permissions_are_scoped_cwd_guard_and_plan_is_read_only() {
        let fixture = Fixture::new();
        let mut item = fixture.record();
        item.state = "queued".into();
        item.thread_id = Some(uuid::Uuid::new_v4().to_string());
        item.source.permission_mode = Some(PermissionMode::Plan);
        let mut target = item.source.clone();
        target.agent_runtime_kind = Some("codex".into());
        let mut command = Command::new("fake-codex");
        configure_target_record(&mut command, &target, &item).unwrap();
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(
            args,
            vec![
                "resume",
                item.thread_id.as_deref().unwrap(),
                "--sandbox",
                "read-only",
                "--ask-for-approval",
                "on-request"
            ]
        );
        item.thread_id = None;
        item.source.permission_mode = Some(PermissionMode::Default);
        let mut writable = Command::new("fake-codex");
        configure_target_record(&mut writable, &target, &item).unwrap();
        assert_eq!(
            writable.get_args().collect::<Vec<_>>(),
            [
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "on-request"
            ]
        );
        target.cwd = fixture.root.join("wrong").display().to_string();
        assert!(configure_target_record(&mut Command::new("fake-codex"), &target, &item).is_err());
    }
    #[test]
    fn import_id_is_not_thread_id_early_completed_is_matchable_and_windows_paths_canonicalize() {
        let fixture = Fixture::new();
        let file = fixture.root.join("fake.jsonl");
        std::fs::write(&file, "{}\n").unwrap();
        let thread = uuid::Uuid::new_v4().to_string();
        let value = serde_json::json!({"method":"externalAgentConfig/import/completed","params":{
            "importId":"import-1","itemTypeResults":[{"itemType":"SESSIONS","failures":[],"successes":[{
                "source":file.canonicalize().unwrap().display().to_string(),"cwd":fixture.root.canonicalize().unwrap().display().to_string(),"target":thread}]}]}});
        assert!(completion_matches(&value, "import-1"));
        assert!(!completion_matches(&value, "other-import"));
        assert_eq!(
            imported_thread(
                &value,
                &file.display().to_string(),
                &fixture.root.display().to_string()
            )
            .unwrap(),
            thread
        );
        assert!(imported_thread(
            &serde_json::json!({"result":{"importId":"import-1"}}),
            "",
            ""
        )
        .is_err());
        let mut skipped = value.clone();
        skipped["params"]["itemTypeResults"][0]["successes"] = serde_json::json!([]);
        assert!(imported_thread(
            &skipped,
            &file.display().to_string(),
            &fixture.root.display().to_string()
        )
        .is_err());
        let mut failed = value;
        failed["params"]["itemTypeResults"][0]["failures"] =
            serde_json::json!([{"message":"fixture importer failure"}]);
        assert!(imported_thread(
            &failed,
            &file.display().to_string(),
            &fixture.root.display().to_string()
        )
        .is_err());
    }
    #[test]
    fn fallback_has_total_bound_and_preserves_recent_user_request() {
        let messages = (0..200)
            .map(|index| crate::transcript::TranscriptMessage {
                role: "user".into(),
                text: format!("request-{index} {}", "中".repeat(3000)),
                at: None,
            })
            .collect::<Vec<_>>();
        let result = bounded_history(&"git".repeat(5000), &messages);
        assert!(result.chars().count() <= 31000);
        assert!(result.contains("request-199"));
        assert!(!result.contains("request-0 "));
    }
    struct SwitchDirectory;
    impl Drop for SwitchDirectory {
        fn drop(&mut self) {
            TEST_SWITCH_DIR.with(|slot| *slot.borrow_mut() = None);
            crate::session_switch_permissions::TEST_HOME.with(|slot| *slot.borrow_mut() = None);
        }
    }
    fn isolate_switches(root: &std::path::Path) -> SwitchDirectory {
        TEST_SWITCH_DIR.with(|slot| *slot.borrow_mut() = Some(root.into()));
        crate::session_switch_permissions::TEST_HOME
            .with(|slot| *slot.borrow_mut() = Some(root.into()));
        SwitchDirectory
    }
    fn native_app(fixture: &Fixture) -> tauri::App<tauri::test::MockRuntime> {
        use crate::native_acp_sessions::*;
        let peer = |name: &str| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../tests/fixtures")
                .join(name)
        };
        let manager = |flavor, name: &str, file: &str| {
            let mut manager =
                NativeAcpSessions::open_at_flavor(fixture.root.join(file), flavor).unwrap();
            manager.test_peer = Some(peer(name));
            manager
        };
        let app = tauri::test::mock_app();
        app.manage(fixture.sessions.clone());
        app.manage(manager(
            NativeChatFlavor::DeepSeekHarness,
            "native-harness-acp.mjs",
            "deepseek.json",
        ));
        app.manage(NativeClaudeSessions(manager(
            NativeChatFlavor::ClaudeChat,
            "native-claude-acp.mjs",
            "claude.json",
        )));
        app.manage(NativeCodexChatSessions(manager(
            NativeChatFlavor::CodexChat,
            "codex-acp-agent.mjs",
            "codex.json",
        )));
        app.manage(NativeKimiSessions(manager(
            NativeChatFlavor::KimiChat,
            "kimi-acp-agent.mjs",
            "kimi.json",
        )));
        app.manage(NativeOpenCodeChatSessions(manager(
            NativeChatFlavor::OpenCodeChat,
            "opencode-acp-agent.mjs",
            "opencode.json",
        )));
        let mut pi =
            crate::native_pi_sessions::NativePiSessions::open_at(fixture.root.join("pi.json"))
                .unwrap();
        pi.test_peer = Some(peer("pi-chat-agent.mjs"));
        app.manage(pi);
        let mut zcode = crate::native_zcode_sessions::NativeZCodeSessions::open_at(
            fixture.root.join("zcode.json"),
        )
        .unwrap();
        zcode.test_peer = Some((peer("native-zcode-agent.mjs"), "normal".into()));
        app.manage(zcode);
        app
    }
    fn prepare_fixture<R: tauri::Runtime>(
        fixture: &Fixture,
        app: &tauri::AppHandle<R>,
        source: SessionRecord,
        target: &str,
        logical: &str,
    ) -> SwitchRecord {
        let item = SwitchRecord {
            id: uuid::Uuid::new_v4().to_string(),
            source_pty: source.pty_id,
            source_position: 0,
            source_permission: actual_permission(app, &source).unwrap(),
            source,
            target_id: None,
            thread_id: None,
            target_runtime: target.into(),
            logical_id: Some(logical.into()),
            source_history: vec![],
            preserve_source: true,
            recovery_dismissed: false,
            state: "preview".into(),
            history_mode: "handoff".into(),
            context: "fixture".into(),
            error: None,
        };
        save(&fixture.db, &item).unwrap();
        mark_managed(&item.source.id).unwrap();
        item
    }
    #[test]
    fn retained_source_roundtrip_and_undo_preserve_visibility_and_execution_fences() {
        let fixture = Fixture::new();
        let _directory = isolate_switches(&fixture.root);
        let app = native_app(&fixture);
        let source = fixture.sessions.create_runtime(NewRuntimeSession {
            runtime_kind: "claude_chat".into(), cwd: fixture.root.display().to_string(), project_id: None,
        }).unwrap();
        crate::session_switch_native::start(app.handle(), &source).unwrap();
        let mut item = prepare_fixture(&fixture, app.handle(), source.clone(), "codex_chat", &source.id);
        let ptys = PtyManager::new();
        let queue = SessionActionQueue::new();
        execute(&fixture.db, &mut item, true, &fixture.sessions, &ptys, &queue, Some(app.handle())).unwrap();
        let loaded = records(&fixture.db).unwrap().into_iter().find(|row|row.id == item.id).unwrap();
        assert!(loaded.preserve_source);
        let visible = fixture.sessions.list();
        assert!(visible.iter().any(|row|row.id == source.id));
        assert!(visible.iter().any(|row|Some(&row.id) == item.target_id.as_ref()));
        assert!(check_start_records(&[loaded.clone()], &source.id, false, None).is_err());
        let old = { let mut value=serde_json::to_value(&loaded).unwrap(); value.as_object_mut().unwrap().remove("preserveSource"); serde_json::from_value::<SwitchRecord>(value).unwrap() };
        assert!(!old.preserve_source, "legacy records keep the former layout");
        let cancelled = restore_switch(&item.id, &fixture.sessions, &ptys, &queue, app.handle()).unwrap();
        assert_eq!(cancelled.state, "cancelled");
        assert!(fixture.sessions.get(&source.id).unwrap().archived_at.is_none());
        assert!(fixture.sessions.get(item.target_id.as_deref().unwrap()).unwrap().archived_at.is_some());
        assert!(check_start_records(&[cancelled], &source.id, false, None).is_ok());
        crate::session_switch_native::start(app.handle(), &fixture.sessions.get(&source.id).unwrap()).unwrap();
        assert!(app.state::<crate::native_acp_sessions::NativeClaudeSessions>().0.snapshot(&source.id).unwrap().connected);
        crate::session_switch_native::stop(app.handle(), &fixture.sessions.get(&source.id).unwrap()).unwrap();
    }
    #[test]
    #[ignore = "需要已安装 Codex CLI；使用隔离原生 Claude Chat fixture 验证真实交接预览入口，不确认交接"]
    fn handoff_preflight_native_chat_preview_preserves_running_source_until_confirmation() {
        let fixture = Fixture::new();
        let _directory = isolate_switches(&fixture.root);
        let app = native_app(&fixture);
        let source = fixture.sessions.create_runtime(NewRuntimeSession {
            runtime_kind: "claude_chat".into(), cwd: fixture.root.display().to_string(), project_id: None,
        }).unwrap();
        crate::session_switch_native::start(app.handle(), &source).unwrap();
        let before = fixture.sessions.get(&source.id).unwrap();
        let result = prepare_switch_preview(source.id.clone(), Some(2), Some("codex_chat".into()), app.handle(), &fixture.sessions);
        let after = fixture.sessions.get(&source.id).unwrap();
        let count = fixture.sessions.list().len();
        app.state::<crate::native_acp_sessions::NativeClaudeSessions>().0.stop(&source.id, &fixture.sessions, app.handle()).unwrap();
        let preview = result.unwrap();
        assert_eq!(preview.state, "preview");
        assert_eq!(preview.target_runtime, "codex_chat");
        assert_eq!(preview.source_position, 2);
        assert!(preview.target_id.is_none());
        assert_eq!(count, 1, "preview must not create/start the receiver");
        assert_eq!(after.status, before.status, "preview must not stop the source");
        assert_eq!(after.archived_at, before.archived_at);
    }

    #[test]
    fn native_chat_fixture_all_seven_cross_agent_chain_stops_sources_and_preserves_latest_context_once(
    ) {
        let fixture = Fixture::new();
        let _directory = isolate_switches(&fixture.root);
        let app = native_app(&fixture);
        let ptys = PtyManager::new();
        let queue = SessionActionQueue::new();
        let initial = fixture
            .sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "claude_chat".into(),
                cwd: fixture.root.display().to_string(),
                project_id: None,
            })
            .unwrap();
        crate::session_switch_native::start(app.handle(), &initial).unwrap();
        let logical = initial.id.clone();
        let mut current = initial;
        for (index, target) in [
            "codex_chat",
            "kimi_chat",
            "opencode_chat",
            "pi_chat",
            "zcode",
            "deepseek_harness",
            "claude_chat",
        ]
        .iter()
        .enumerate()
        {
            // Source has an explicit most-recent user instruction, independent of any model summary.
            let marker = uuid::Uuid::new_v4().to_string();
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
            let instruction = format!(
                "[CrewVela handoff {marker}]\n接力用户任务-{index}，不要遗漏结尾验收-{index}"
            );
            loop {
                match crate::session_switch_native::send(
                    app.handle(),
                    &current,
                    &marker,
                    &instruction,
                ) {
                    Ok(()) => break,
                    Err(error) => {
                        assert!(
                            std::time::Instant::now() < deadline,
                            "source send failed: {error}"
                        );
                        std::thread::sleep(std::time::Duration::from_millis(20));
                    }
                }
            }
            let source = fixture.sessions.get(&current.id).unwrap();
            let mut item = prepare_fixture(&fixture, app.handle(), source, target, &logical);
            execute(
                &fixture.db,
                &mut item,
                true,
                &fixture.sessions,
                &ptys,
                &queue,
                Some(app.handle()),
            )
            .unwrap();
            assert_eq!(item.state, "queued");
            assert!(
                item.context.contains(&format!("验收-{index}")),
                "{}",
                item.context
            );
            let retained = fixture.sessions.get(&current.id).unwrap();
            assert!(retained.archived_at.is_none(), "new handoffs retain the source in the normal session list");
            assert_eq!(retained.status, crate::sessions::SessionStatus::Stopped);
            assert!(retained.pty_id.is_none());
            assert!(fixture.sessions.list().iter().any(|source| source.id == retained.id));
            assert!(
                check_start_records(&records(&fixture.db).unwrap(), &current.id, false, None)
                    .is_err()
            );
            let next = fixture
                .sessions
                .get(item.target_id.as_deref().unwrap())
                .unwrap();
            assert_eq!(next.cwd, current.cwd);
            assert!(!next.yolo);
            assert!(next.worktree.is_none());
            execute(
                &fixture.db,
                &mut item,
                true,
                &fixture.sessions,
                &ptys,
                &queue,
                Some(app.handle()),
            )
            .unwrap();
            assert!(
                crate::session_switch_native::has_submission(app.handle(), &next, &item.id)
                    .unwrap()
            );
            if let Some(manager) = match target.as_ref() {
                "claude_chat" => Some(
                    app.state::<crate::native_acp_sessions::NativeClaudeSessions>()
                        .0
                        .clone(),
                ),
                "codex_chat" => Some(
                    app.state::<crate::native_acp_sessions::NativeCodexChatSessions>()
                        .0
                        .clone(),
                ),
                "kimi_chat" => Some(
                    app.state::<crate::native_acp_sessions::NativeKimiSessions>()
                        .0
                        .clone(),
                ),
                "opencode_chat" => Some(
                    app.state::<crate::native_acp_sessions::NativeOpenCodeChatSessions>()
                        .0
                        .clone(),
                ),
                "deepseek_harness" => Some(
                    app.state::<crate::native_acp_sessions::NativeAcpSessions>()
                        .inner()
                        .clone(),
                ),
                _ => None,
            } {
                let snapshot = manager.snapshot(&next.id).unwrap();
                assert_eq!(
                    snapshot
                        .history
                        .iter()
                        .filter(|m| m.role == "user" && m.text.contains(&item.id))
                        .count(),
                    1
                );
                // Release fake-tool approvals so the next explicit user task can start.
                let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
                loop {
                    let view = manager.snapshot(&next.id).unwrap();
                    if let Some(permission) = view.pending_permission {
                        if let Err(error) = manager.answer_permission(
                            &next.id,
                            permission["requestId"].as_str().unwrap(),
                            false,
                        ) {
                            assert!(error.to_string().contains("已结束或不存在"), "{error}");
                            // Permission display is a snapshot: a worker can retire
                            // the request before the response reaches it. Require
                            // it to disappear rather than silently ignoring failure.
                            let request = permission["requestId"].clone();
                            while manager
                                .snapshot(&next.id)
                                .unwrap()
                                .pending_permission
                                .as_ref()
                                .is_some_and(|pending| pending["requestId"] == request)
                            {
                                assert!(
                                    std::time::Instant::now() < until,
                                    "stale permission did not retire: {error}"
                                );
                                std::thread::sleep(std::time::Duration::from_millis(20));
                            }
                        }
                    }
                    if !view.busy {
                        break;
                    }
                    assert!(
                        std::time::Instant::now() < until,
                        "fake target did not settle"
                    );
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            } else if *target == "pi_chat" {
                let manager = app.state::<crate::native_pi_sessions::NativePiSessions>();
                let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
                loop {
                    let view = manager.snapshot(&next.id).unwrap();
                    if let Some(permission) = view.pending_permission {
                        manager
                            .answer_permission(
                                &next.id,
                                permission["requestId"].as_str().unwrap(),
                                false,
                            )
                            .unwrap();
                    }
                    if !view.busy {
                        break;
                    }
                    assert!(std::time::Instant::now() < until);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            } else {
                let until = std::time::Instant::now() + std::time::Duration::from_secs(8);
                while app
                    .state::<crate::native_zcode_sessions::NativeZCodeSessions>()
                    .snapshot(&next.id)
                    .unwrap()
                    .busy
                {
                    assert!(std::time::Instant::now() < until);
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
            current = next;
        }
        let items = records(&fixture.db).unwrap();
        assert!(ensure_latest_transfer(&items[0], &items).is_err());
        assert!(ensure_latest_transfer(items.last().unwrap(), &items).is_ok());
        crate::session_switch_native::stop(app.handle(), &current).unwrap();
    }
    #[test]
    fn zcode_cli_fixture_handoff_uses_cli_native_protocol_and_fixed_ack_identity() {
        let fixture = Fixture::new();
        let _directory = isolate_switches(&fixture.root);
        let app = native_app(&fixture);
        let source = fixture.record().source;
        let logical = source.id.clone();
        let mut item = prepare_fixture(&fixture, app.handle(), source, "zcode_cli", &logical);
        execute(
            &fixture.db,
            &mut item,
            true,
            &fixture.sessions,
            &PtyManager::new(),
            &SessionActionQueue::new(),
            Some(app.handle()),
        )
        .unwrap();
        let target = fixture
            .sessions
            .get(item.target_id.as_deref().unwrap())
            .unwrap();
        assert_eq!(target.agent_runtime_kind.as_deref(), Some("zcode_cli"));
        assert!(target.pty_id.is_none());
        assert!(
            crate::session_switch_native::has_submission(app.handle(), &target, &item.id).unwrap()
        );
        execute(
            &fixture.db,
            &mut item,
            true,
            &fixture.sessions,
            &PtyManager::new(),
            &SessionActionQueue::new(),
            Some(app.handle()),
        )
        .unwrap();
        let view = app
            .state::<crate::native_zcode_sessions::NativeZCodeSessions>()
            .snapshot(&target.id)
            .unwrap();
        assert_eq!(
            view.rows
                .iter()
                .filter(|row| row["kind"] == "userInput"
                    && row["text"]
                        .as_str()
                        .is_some_and(|text| text.contains(&item.id)))
                .count(),
            1
        );
        crate::session_switch_native::stop(app.handle(), &target).unwrap();
    }
    #[test]
    fn legacy_record_defaults_and_latest_long_user_instruction_are_preserved() {
        let fixture = Fixture::new();
        let item = fixture.record();
        let mut value = serde_json::to_value(&item).unwrap();
        for field in [
            "targetRuntime",
            "logicalId",
            "sourceHistory",
            "sourcePermission",
        ] {
            value.as_object_mut().unwrap().remove(field);
        }
        let legacy: SwitchRecord = serde_json::from_value(value).unwrap();
        assert_eq!(legacy.target_runtime, "codex");
        assert!(legacy.source_permission.is_none());
        let text = format!("{}关键结尾约束", "长指令".repeat(3500));
        let result = bounded_history(
            "git",
            &[
                crate::transcript::TranscriptMessage {
                    role: "user".into(),
                    text: text.clone(),
                    at: None,
                },
                crate::transcript::TranscriptMessage {
                    role: "system".into(),
                    text: "terminal".repeat(20000),
                    at: None,
                },
            ],
        );
        assert!(result.contains(&text));
        assert!(result.chars().count() < 31000);
    }
    #[test]
    fn interrupted_acp_submission_never_promotes_local_history_to_remote_receipt() {
        let fixture = Fixture::new();
        let _directory = isolate_switches(&fixture.root);
        let app = native_app(&fixture);
        let source = fixture
            .sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "kimi_chat".into(),
                cwd: fixture.root.display().to_string(),
                project_id: None,
            })
            .unwrap();
        crate::session_switch_native::start(app.handle(), &source).unwrap();
        let logical = source.id.clone();
        let mut item = prepare_fixture(&fixture, app.handle(), source, "claude_chat", &logical);
        let ptys = PtyManager::new();
        let queue = SessionActionQueue::new();
        execute(
            &fixture.db,
            &mut item,
            true,
            &fixture.sessions,
            &ptys,
            &queue,
            Some(app.handle()),
        )
        .unwrap();
        let target = fixture
            .sessions
            .get(item.target_id.as_deref().unwrap())
            .unwrap();
        assert!(
            crate::session_switch_native::has_submission(app.handle(), &target, &item.id).unwrap()
        );
        // Simulate a durable sending record after an interrupted process. Even
        // a persisted local user message does not prove native admission.
        crate::session_switch_native::stop(app.handle(), &target).unwrap();
        item.state = "sending".into();
        save(&fixture.db, &item).unwrap();
        assert!(execute(
            &fixture.db,
            &mut item,
            true,
            &fixture.sessions,
            &ptys,
            &queue,
            Some(app.handle())
        )
        .is_err());
        assert_eq!(item.state, "sending");
        let view = app
            .state::<crate::native_acp_sessions::NativeClaudeSessions>()
            .0
            .snapshot(&target.id)
            .unwrap();
        assert_eq!(
            view.history
                .iter()
                .filter(|message| message.role == "user"
                    && message
                        .text
                        .starts_with(&format!("[CrewVela handoff {}]", item.id)))
                .count(),
            1
        );
    }
}
