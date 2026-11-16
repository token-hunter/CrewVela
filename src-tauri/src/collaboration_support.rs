//! 协作诊断及显式交接。预览仅收集证据；确认才会停止或改变所有权。
use std::path::Path;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::State;

use crate::control::{QueuedMessage, SessionActionQueue};
use crate::multi_agent::{MultiAgentStore, MultiAgentSupervisor};
use crate::pty::PtyManager;
use crate::sessions::{SessionKind, SessionManager, SessionRecord};

const BUILTIN_SKILL: &str = include_str!("../../skills/crewvela/SKILL.md");
static HANDOFF_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffInput {
    pub mode: String,
    pub project_id: Option<String>,
    pub source_id: String,
    pub target_id: String,
    pub goal: String,
    pub progress: String,
    pub remaining: String,
    pub blockers: String,
    pub files: String,
    pub verification: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffDraft {
    pub id: String,
    pub mode: String,
    pub project_id: Option<String>,
    pub source_id: String,
    pub target_id: String,
    pub source_pty: Option<u32>,
    pub target_pty: Option<u32>,
    pub source_identity: String,
    pub target_identity: String,
    pub original_agent: Option<String>,
    pub context: String,
    pub state: String,
    pub error: Option<String>,
    pub consequence: String,
}

fn database() -> Result<Connection> {
    let connection =
        Connection::open(crate::providers::config_dir()?.join("collaboration-handoffs.sqlite"))?;
    connection.execute_batch("CREATE TABLE IF NOT EXISTS handoffs(id TEXT PRIMARY KEY, draft_json TEXT NOT NULL, updated_at INTEGER NOT NULL);")?;
    Ok(connection)
}

fn save(connection: &Connection, draft: &HandoffDraft) -> Result<()> {
    connection.execute("INSERT INTO handoffs(id, draft_json, updated_at) VALUES (?1, ?2, unixepoch()) ON CONFLICT(id) DO UPDATE SET draft_json = excluded.draft_json, updated_at = excluded.updated_at",
        params![draft.id, serde_json::to_string(draft)?])?;
    Ok(())
}

fn identity(record: &SessionRecord) -> String {
    format!(
        "{:?}|{:?}|{}|{}|{}",
        record.kind,
        record.agent_runtime_kind,
        record.cwd,
        record.claude_session_id,
        record.runtime_command_host
    )
}

fn runtime(record: &SessionRecord) -> Option<&str> {
    record
        .agent_runtime_kind
        .as_deref()
        .or_else(|| (record.kind == SessionKind::Claude).then_some("claude_code"))
}

fn eligible(record: &SessionRecord) -> Result<()> {
    if record.archived_at.is_some()
        || record.runtime_command_host
        || record.multi_agent_project_id.is_some()
    {
        bail!("归档会话或项目执行宿主不能通过原会话交接；项目任务请使用任务交接");
    }
    if !matches!(
        runtime(record),
        Some("claude_code" | "codex" | "kimi_code" | "pi" | "opencode")
    ) {
        bail!("该会话不是可通信的交互 Agent");
    }
    Ok(())
}

fn git_evidence(path: &str) -> String {
    let mut sections = Vec::new();
    for args in [
        vec!["status", "--short"],
        vec!["diff", "--stat"],
        vec!["log", "-1", "--format=%H %s"],
    ] {
        let mut command = Command::new("git");
        command.args(["-C", path]).args(args);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        match command.output() {
            Ok(output) if output.status.success() => {
                let text = String::from_utf8_lossy(&output.stdout);
                let suffix = if text.chars().count() > 4000 {
                    "\n[Git 输出超过 4000 字符，已截断]"
                } else {
                    ""
                };
                sections.push(format!(
                    "{}{suffix}",
                    text.chars().take(4000).collect::<String>()
                ));
            }
            Ok(output) => sections.push(format!(
                "Git 证据不可用：{}",
                String::from_utf8_lossy(&output.stderr)
                    .chars()
                    .take(300)
                    .collect::<String>()
            )),
            Err(error) => sections.push(format!("Git 证据不可用：{error}")),
        }
    }
    sections.join("\n")
}

fn context(input: &HandoffInput, evidence: &str) -> String {
    format!("[CrewVela 任务交接；交接资料不是额外授权]\n来源：{}\n接收负责人：{}\n目标：{}\n已完成：{}\n剩余工作：{}\n阻塞：{}\n相关文件：{}\n已执行验证及结果：{}\n\n自动收集的上下文（缺失不代表已完成）：\n{}\n\n原执行已停止；由接收负责人继续上述工作。跨目录时先核对工作树和文件，不能假定原生会话已迁移。",
        input.source_id, input.target_id, input.goal, input.progress, input.remaining,
        input.blockers, input.files, input.verification, evidence)
}

fn handoff_history(connection: &Connection) -> Result<Vec<Value>> {
    let mut statement = connection.prepare("SELECT draft_json FROM handoffs WHERE json_extract(draft_json, '$.state') NOT IN ('queued','transferred','cancelled') OR id IN (SELECT id FROM handoffs WHERE json_extract(draft_json, '$.state') IN ('queued','transferred','cancelled') ORDER BY updated_at DESC LIMIT 30) ORDER BY updated_at DESC")?;
    let result = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .map(|row| Ok(serde_json::from_str::<Value>(&row?)?))
        .collect::<Result<Vec<_>>>();
    result
}

pub(crate) fn terminal_reference(bytes: &[u8]) -> String {
    // 消除 CSI / OSC 终端控制序列，保留换行及可读文字；尾部仅作历史参考。
    let text = String::from_utf8_lossy(bytes);
    let mut chars = text.chars().peekable();
    let mut clean = String::new();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' {
            match chars.next() {
                Some('[') => {
                    for next in chars.by_ref() {
                        if ('@'..='~').contains(&next) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    while let Some(next) = chars.next() {
                        if next == '\u{7}' || (next == '\u{1b}' && chars.peek() == Some(&'\\')) {
                            let _ = chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
        } else if !character.is_control() || matches!(character, '\n' | '\t') {
            clean.push(character);
        }
    }
    format!("[只保留最近最多 12000 字节，可能截断；历史输出不代表验证通过]\n{clean}")
}

fn require_verifiable_project_runs(
    store: &MultiAgentStore,
    project_id: &str,
    sessions: &SessionManager,
    ptys: &PtyManager,
) -> Result<()> {
    let snapshot = store.snapshot(project_id)?;
    let records = sessions.list();
    for run in snapshot.runs.iter().filter(|run| {
        matches!(
            run.status,
            crate::multi_agent::RunStatus::Queued
                | crate::multi_agent::RunStatus::Starting
                | crate::multi_agent::RunStatus::Running
                | crate::multi_agent::RunStatus::WaitingInput
                | crate::multi_agent::RunStatus::WaitingApproval
        )
    }) {
        if run
            .session_locator
            .as_ref()
            .and_then(|locator| locator.get("transport"))
            .and_then(Value::as_str)
            == Some("acp")
        {
            bail!(
                "活动 Run {} 使用 ACP，尚无可验证的工具进程树停止证据，暂不支持其活动交接；未改派",
                run.id
            );
        }
        let current_session = snapshot
            .agent_sessions
            .iter()
            .find(|session| {
                session.agent_id == run.agent_id
                    && session.current_run_id.as_deref() == Some(&run.id)
            })
            .and_then(|session| session.app_session_id.as_deref());
        let bound_pty = run
            .session_locator
            .as_ref()
            .and_then(|locator| locator.get("ptyId"))
            .and_then(Value::as_u64);
        let transport = run
            .session_locator
            .as_ref()
            .and_then(|locator| locator.get("transport"))
            .and_then(Value::as_str);
        if !records.iter().any(|record| {
            Some(record.id.as_str()) == current_session
                && record.multi_agent_project_id.as_deref() == Some(project_id)
                && transport == Some("pty")
                && record.pty_id.map(u64::from) == bound_pty
                && record.pty_id.is_some_and(|id| ptys.is_running(id))
        }) {
            bail!("活动 Run {} 没有可验证的 PTY 进程树（可能为 ACP 或尚在启动）；暂不支持其活动交接，未暂停或改派", run.id);
        }
    }
    #[cfg(not(windows))]
    if snapshot.runs.iter().any(|run| {
        matches!(
            run.status,
            crate::multi_agent::RunStatus::Running | crate::multi_agent::RunStatus::Starting
        )
    }) {
        bail!("当前平台未提供可验证进程树停止，不能交接活动任务");
    }
    Ok(())
}

#[tauri::command]
pub fn collaboration_health(
    sessions: State<'_, SessionManager>,
    ptys: State<'_, PtyManager>,
    queue: State<'_, SessionActionQueue>,
) -> Result<Value, String> {
    health(sessions.inner(), ptys.inner(), queue.inner()).map_err(|error| error.to_string())
}

fn health(
    sessions: &SessionManager,
    ptys: &PtyManager,
    queue: &SessionActionQueue,
) -> Result<Value> {
    let records = sessions.list();
    let mut adapters = crate::skill_center::runtime_adapters()?;
    // Builtin communication also supports ZCode CLI; this does not claim that
    // Skill Center supports its general deployment/update workflow.
    let native_roots = crate::communication_skill::runtime_roots()?;
    for adapter in &mut adapters {
        if let Some((_, root)) = native_roots.iter().find(|(runtime, _)| *runtime == adapter.runtime) {
            adapter.global_root = root.display().to_string();
        }
    }
    let zcode_root = native_roots.iter().find(|(runtime, _)| *runtime == "zcode_cli")
        .context("缺少 ZCode CLI 通信 Skill 目录")?;
    let home = dirs::home_dir().context("无法确定通信 Skill 目录")?;
    adapters.push(crate::skill_center::RuntimeSkillAdapter {
        runtime: "zcode_cli".into(),
        label: "ZCode CLI".into(),
        global_root: zcode_root.1.display().to_string(),
        project_root: ".zcode/skills".into(),
        discovery_roots: vec![home.join(".agents/skills").display().to_string()],
        recursive_scan: false,
        installed: crate::multi_agent::resolve_runtime_executable(crate::multi_agent::AgentRuntimeKind::ZCodeCli).is_some(),
    });
    // OpenCode 也发现兼容的 Claude / Agents Skill 根；复用已有 adapter 的根定义。
    let shared = adapters
        .iter()
        .filter(|adapter| matches!(adapter.runtime.as_str(), "claude_code" | "codex"))
        .flat_map(|adapter| {
            std::iter::once(adapter.global_root.clone()).chain(adapter.discovery_roots.clone())
        })
        .collect::<Vec<_>>();
    if let Some(adapter) = adapters
        .iter_mut()
        .find(|adapter| adapter.runtime == "opencode")
    {
        adapter.discovery_roots.extend(shared);
    }
    let mut runtimes = adapters.into_iter().map(|adapter| {
        let mut roots = vec![adapter.global_root.clone()];
        roots.extend(adapter.discovery_roots);
        let global_roots = roots.clone();
        for record in records.iter().filter(|record| record.archived_at.is_none() && runtime(record) == Some(adapter.runtime.as_str())) {
            roots.push(Path::new(&record.cwd).join(&adapter.project_root).display().to_string());
        }
        roots.sort(); roots.dedup();
        let skills = roots.iter().filter_map(|root| {
            let path = Path::new(root).join("crewvela/SKILL.md");
            if !path.exists() { return None; }
            let project_ids = if global_roots.contains(root) { Vec::new() } else {
                records.iter().filter(|record| record.archived_at.is_none() && runtime(record) == Some(adapter.runtime.as_str()) && Path::new(&record.cwd).join(&adapter.project_root).display().to_string() == *root)
                    .filter_map(|record| record.multi_agent_project_id.clone()).collect::<Vec<_>>()
            };
            let session_ids = records.iter().filter(|record| record.archived_at.is_none() && runtime(record) == Some(adapter.runtime.as_str()) && Path::new(&record.cwd).join(&adapter.project_root).display().to_string() == *root)
                .map(|record| record.id.clone()).collect::<Vec<_>>();
            Some(match std::fs::read_to_string(&path) {
                Ok(content) => json!({"path": path.display().to_string(), "globalRoot": global_roots.contains(root), "projectIds": project_ids, "sessionIds": session_ids, "matches": normalized(&content) == normalized(BUILTIN_SKILL), "error": null}),
                Err(error) => json!({"path": path.display().to_string(), "globalRoot": global_roots.contains(root), "projectIds": project_ids, "sessionIds": session_ids, "matches": false, "error": error.to_string()}),
            })
        }).collect::<Vec<_>>();
        json!({"runtime": adapter.runtime, "label": adapter.label, "installed": adapter.installed,
            "skills": skills, "searchedRoots": roots, "supported": true})
    }).collect::<Vec<_>>();
    for (kind, label) in [("zcode", "ZCode Desktop"), ("deepseek_harness", "DeepSeek Harness")] {
        runtimes.push(json!({"runtime": kind, "label": label, "installed": false, "skills": [], "searchedRoots": [], "supported": false}));
    }
    let views = records.iter().filter(|record| runtime(record).is_some()).map(|record| {
        let process_running = record.pty_id.is_some_and(|id| ptys.is_running(id));
        let ready = process_running && !record.runtime_command_host && sessions.is_ready_for_prompt(&record.id);
        let reason = if record.runtime_command_host { "项目后台轮次宿主，不能直接注入聊天" }
            else if !process_running { "未观察到当前 PTY 进程运行；旧 ID 不会自动转发" }
            else if ready { "已观察到可接收提示词状态；尚未测试消息往返" }
            else { "尚未观察到输入就绪；可能执行中、授权等待或启动中" };
        json!({"id": record.id, "title": record.title, "runtime": runtime(record), "cwd": record.cwd,
            "commandHost": record.runtime_command_host, "projectId": record.multi_agent_project_id, "archivedAt": record.archived_at, "status": record.status,
            "ptyId": record.pty_id, "processRunning": process_running, "ready": ready, "reason": reason,
            "error": record.last_error, "waitingReason": record.waiting_reason,
            "bridge": sessions.communication_bridge_evidence(&record.id),
            "identityEnvironment": "未知：启动逻辑注入，未读取子进程环境确认",
            "interactiveHandoff": eligible(record).is_ok() && process_running})
    }).collect::<Vec<_>>();
    let connection = database()?;
    let handoffs = handoff_history(&connection)?;
    Ok(
        json!({"cliPath": std::env::current_exe()?.display().to_string(),
        "cliPathExists": std::env::current_exe()?.is_file(), "runtimes": runtimes, "sessions": views,
        "messages": queue.communication_evidence(), "handoffs": handoffs,
        "replyEvidence": "未提供消息级回复关联证据；不将任意终端输出标为已回复"}),
    )
}

fn normalized(text: &str) -> String {
    text.replace("\r\n", "\n").trim().to_string()
}

#[tauri::command]
pub fn collaboration_handoff_cancel(
    id: String,
    store: State<'_, MultiAgentStore>,
    queue: State<'_, SessionActionQueue>,
) -> Result<(), String> {
    let _guard = HANDOFF_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    (|| -> Result<()> {
        let connection = database()?;
        let raw: String = connection.query_row(
            "SELECT draft_json FROM handoffs WHERE id = ?1",
            [&id],
            |row| row.get(0),
        )?;
        let mut draft: HandoffDraft = serde_json::from_str(&raw)?;
        if queue.handoff_enqueued(&id) || store.task_handoff_completed(&id)? {
            bail!("交接已经持久化入队或变更负责人，不能取消；请检查消息与任务状态");
        }
        if matches!(draft.state.as_str(), "queued" | "transferred") {
            bail!("已移交的任务不能撤销所有权");
        }
        if let Some(project) = &draft.project_id {
            store.release_handoff_lock(project, &draft.id)?;
        }
        draft.state = "cancelled".into();
        draft.error =
            Some("交接已取消；已暂停的项目或已停止的会话不会自动恢复，上下文仍保留".into());
        save(&connection, &draft)
    })()
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn collaboration_handoff_preview(
    input: HandoffInput,
    sessions: State<'_, SessionManager>,
    ptys: State<'_, PtyManager>,
    store: State<'_, MultiAgentStore>,
) -> Result<HandoffDraft, String> {
    preview(input, sessions.inner(), ptys.inner(), store.inner()).map_err(|error| error.to_string())
}

fn preview(
    input: HandoffInput,
    sessions: &SessionManager,
    ptys: &PtyManager,
    store: &MultiAgentStore,
) -> Result<HandoffDraft> {
    if input.source_id == input.target_id
        || input.goal.trim().is_empty()
        || input.remaining.trim().is_empty()
    {
        bail!("请选择不同接收方，并填写目标与剩余工作");
    }
    let mut draft = HandoffDraft {
        id: uuid::Uuid::new_v4().to_string(),
        mode: input.mode.clone(),
        project_id: input.project_id.clone(),
        source_id: input.source_id.clone(),
        target_id: input.target_id.clone(),
        source_pty: None,
        target_pty: None,
        source_identity: String::new(),
        target_identity: String::new(),
        original_agent: None,
        context: String::new(),
        state: "preview".into(),
        error: None,
        consequence: String::new(),
    };
    let evidence = match input.mode.as_str() {
        "session" => {
            let source = sessions.get(&input.source_id).context("原会话不存在")?;
            let target = sessions.get(&input.target_id).context("接收会话不存在")?;
            eligible(&source)?;
            eligible(&target)?;
            let source_pty = source
                .pty_id
                .filter(|id| ptys.is_running(*id))
                .context("原会话不在运行")?;
            let target_pty = target
                .pty_id
                .filter(|id| ptys.is_running(*id))
                .context("接收会话不在运行")?;
            draft.source_pty = Some(source_pty);
            draft.target_pty = Some(target_pty);
            draft.source_identity = identity(&source);
            draft.target_identity = identity(&target);
            draft.consequence = "确认后停止原会话的整个进程树，再向所选现有会话排队交接包。原待收消息保留；不要恢复原会话继续已交接任务。停止失败不投递，不自动重启。".into();
            let tail = ptys.transcript_tail(source_pty, 12000)?;
            format!("来源目录/工作树：{}\n接收目录：{}\nGit 状态/差异统计/最近提交：\n{}\n原终端尾部（可能包含未完成工具输出）：\n{}",
                source.cwd, target.cwd, git_evidence(&source.cwd), terminal_reference(&tail))
        }
        "task" => {
            let project_id = input.project_id.as_deref().context("请选择项目")?;
            let snapshot = store.snapshot(project_id)?;
            require_verifiable_project_runs(store, project_id, sessions, ptys)?;
            let task = snapshot
                .tasks
                .iter()
                .find(|task| task.id == input.source_id)
                .context("任务不属于当前项目")?;
            let target = snapshot
                .agents
                .iter()
                .find(|agent| agent.id == input.target_id && agent.enabled)
                .context("接收成员不可用")?;
            if task.assignee_agent_id.as_deref() == Some(&target.id) {
                bail!("接收成员已经是负责人");
            }
            if matches!(
                task.status,
                crate::multi_agent::TaskStatus::Done
                    | crate::multi_agent::TaskStatus::InReview
                    | crate::multi_agent::TaskStatus::WaitingApproval
            ) {
                bail!("完成或审核中的任务不能交接");
            }
            draft.original_agent = task.assignee_agent_id.clone();
            draft.target_identity = serde_json::to_string(target)?;
            draft.source_identity = task.description.clone();
            draft.consequence = "确认后暂停整个项目（包括其他执行任务和消息轮次），等待原执行退出，再变更本任务负责人并保存交接指令。项目保持暂停，需手动继续；停止超时不改派。".into();
            let artifacts = snapshot
                .artifacts
                .iter()
                .filter(|artifact| artifact.task_id == task.id)
                .collect::<Vec<_>>();
            let runs = snapshot
                .runs
                .iter()
                .filter(|run| run.task_id == task.id)
                .collect::<Vec<_>>();
            let logs = snapshot
                .run_logs
                .iter()
                .filter(|log| log.task_id == task.id)
                .rev()
                .take(15)
                .collect::<Vec<_>>();
            format!("项目目标：{}\n根目录：{}\n任务：{}\n验收：{}\n验证命令（未执行不代表通过）：{:?}\n任务工作区模式：{:?}\n历史 Run/工作树定位：{}\n交付/提交：{}\n最近日志：{}\n根目录 Git 证据：\n{}",
                snapshot.project.goal, snapshot.project.root_path, task.description, task.acceptance_criteria,
                task.verification_command, task.workspace_mode, serde_json::to_string(&runs)?,
                serde_json::to_string(&artifacts)?, serde_json::to_string(&logs)?, git_evidence(&snapshot.project.root_path))
        }
        _ => bail!("不支持的交接类型"),
    };
    draft.context = context(&input, &evidence);
    if draft.context.chars().count() > 32000 {
        bail!("交接上下文超过 32000 字符，请缩短补充内容");
    }
    save(&database()?, &draft)?;
    Ok(draft)
}

#[tauri::command]
pub fn collaboration_handoff_confirm(
    id: String,
    confirmed: bool,
    sessions: State<'_, SessionManager>,
    ptys: State<'_, PtyManager>,
    queue: State<'_, SessionActionQueue>,
    store: State<'_, MultiAgentStore>,
    supervisor: State<'_, MultiAgentSupervisor>,
) -> Result<HandoffDraft, String> {
    if !confirmed {
        return Err("必须确认预览中的停止与所有权变更".into());
    }
    let _guard = HANDOFF_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    let result = (|| -> Result<HandoffDraft> {
        let connection = database()?;
        let raw: String = connection.query_row(
            "SELECT draft_json FROM handoffs WHERE id = ?1",
            [&id],
            |row| row.get(0),
        )?;
        let mut draft: HandoffDraft = serde_json::from_str(&raw)?;
        if draft.state == "cancelled" {
            bail!("交接已取消，请重新预览");
        }
        if queue.handoff_enqueued(&id) {
            draft.state = "queued".into();
            save(&connection, &draft)?;
            return Ok(draft);
        }
        if store.task_handoff_completed(&id)? {
            draft.state = "transferred".into();
            if let Some(project) = &draft.project_id {
                store.release_handoff_lock(project, &id)?;
            }
            save(&connection, &draft)?;
            return Ok(draft);
        }
        if matches!(draft.state.as_str(), "queued" | "transferred") {
            return Ok(draft);
        }
        let outcome = confirm(
            &mut draft,
            &connection,
            sessions.inner(),
            ptys.inner(),
            queue.inner(),
            store.inner(),
            supervisor.inner(),
        );
        if let Err(error) = outcome {
            draft.error = Some(error.to_string());
            save(&connection, &draft)?;
            return Err(anyhow!(
                "{}；交接记录 {} 已保留，可查看上下文后重试",
                error,
                draft.id
            ));
        }
        draft.error = None;
        save(&connection, &draft)?;
        Ok(draft)
    })();
    result.map_err(|error| error.to_string())
}

fn stop_project_tracked_ptys(sessions: &SessionManager, ptys: &PtyManager, project_id: &str) -> Result<()> {
    for record in sessions.list().into_iter().filter(|record| record.multi_agent_project_id.as_deref() == Some(project_id)) {
        let lifecycle = sessions.lifecycle_lock(&record.id);
        let _lifecycle = lifecycle.lock().unwrap();
        let record = sessions.get(&record.id).context("项目会话已经删除")?;
        if let Some(pty_id) = record.pty_id {
            ptys.stop_for_handoff(pty_id)?;
            sessions.mark_stopped(&record.id)?;
        }
    }
    Ok(())
}

fn confirm(
    draft: &mut HandoffDraft,
    connection: &Connection,
    sessions: &SessionManager,
    ptys: &PtyManager,
    queue: &SessionActionQueue,
    store: &MultiAgentStore,
    supervisor: &MultiAgentSupervisor,
) -> Result<()> {
    match draft.mode.as_str() {
        "session" => {
            let (first, second) = if draft.source_id < draft.target_id {
                (&draft.source_id, &draft.target_id)
            } else {
                (&draft.target_id, &draft.source_id)
            };
            let first_lock = sessions.lifecycle_lock(first);
            let second_lock = sessions.lifecycle_lock(second);
            let _first = first_lock.lock().unwrap();
            let _second = second_lock.lock().unwrap();
            let target = sessions.get(&draft.target_id).context("接收会话不存在")?;
            eligible(&target)?;
            if identity(&target) != draft.target_identity
                || target.pty_id != draft.target_pty
                || !target.pty_id.is_some_and(|id| ptys.is_running(id))
            {
                bail!("接收会话已停止或身份变化，请重新预览");
            }
            let source = sessions.get(&draft.source_id).context("原会话不存在")?;
            eligible(&source)?;
            if identity(&source) != draft.source_identity {
                bail!("原会话身份变化，请重新预览");
            }
            if draft.state != "source_stopped" {
                if source.pty_id != draft.source_pty {
                    bail!("原会话执行已变化，请重新预览");
                }
                draft.state = "stopping".into();
                save(connection, draft)?;
                queue.suspend_delivery_for_handoff(&source.id, || {
                    ptys.stop_for_handoff(source.pty_id.context("原会话未运行")?)?;
                    sessions.mark_stopped(&source.id)?;
                    Ok(())
                })?;
                // 不调用 discard：待收队列保留，不自动重启原会话。
                draft.state = "source_stopped".into();
                save(connection, draft)?;
            } else if source.pty_id.is_some() {
                bail!("原会话已被恢复，不能重试交接，请停止并重新预览");
            }
            let mut message = QueuedMessage::new(draft.context.clone(), None);
            message.id = draft.id.clone();
            queue.enqueue_handoff(&target.id, message)?;
            draft.state = "queued".into();
        }
        "task" => {
            let project_id = draft.project_id.as_deref().context("缺少项目")?;
            let snapshot = store.snapshot(project_id)?;
            let target = snapshot
                .agents
                .iter()
                .find(|agent| agent.id == draft.target_id)
                .context("接收成员不存在")?;
            if serde_json::to_string(target)? != draft.target_identity {
                bail!("接收成员配置变化，请重新预览");
            }
            store.begin_task_handoff(&draft.source_id, &draft.id)?;
            require_verifiable_project_runs(store, project_id, sessions, ptys)?;
            let receipt = supervisor.pause(store, project_id)?;
            if !receipt.accepted {
                bail!("{}；负责人保持不变，项目交接锁保留供重试", receipt.message);
            }
            // 一次性执行宿主可能只中断了前台程序，Shell 仍存活；交接前完整退出。
            stop_project_tracked_ptys(sessions, ptys, project_id)?;
            store.finish_task_handoff(
                &draft.source_id,
                &draft.target_id,
                draft.original_agent.as_deref(),
                &draft.id,
                &draft.context,
            )?;
            draft.state = "transferred".into();
        }
        _ => bail!("不支持的交接类型"),
    }
    Ok(())
}

/// 自动起草的交接六字段 + 来源说明。规则摘要（零 LLM）：只做确定性提取，
/// 不做验证结论；用户必须逐项核对后才能生成交接预览。
#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HandoffNotes {
    pub goal: String,
    pub progress: String,
    pub remaining: String,
    pub blockers: String,
    pub files: String,
    pub verification: String,
    pub source: String,
}

/// 单字段字符上限：字段会填进表单文本框，超长只会淹没核对者。
const NOTES_FIELD_CHARS: usize = 900;

fn cap_chars(text: &str, max: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= max {
        return trimmed.to_string();
    }
    format!("{}…", trimmed.chars().take(max).collect::<String>())
}

/// 从工具行内嵌的（可能被截断的）JSON 片段提取字符串字段；未闭合视为无效。
fn json_string_field(snippet: &str, keys: &[&str]) -> Option<String> {
    for key in keys {
        let marker = format!("\"{key}\":");
        let Some(start) = snippet.find(&marker) else {
            continue;
        };
        let rest = snippet[start + marker.len()..].trim_start();
        let mut chars = rest.chars();
        if chars.next() != Some('"') {
            continue;
        }
        let mut value = String::new();
        let (mut escaped, mut closed) = (false, false);
        for character in chars {
            if escaped {
                value.push(match character {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    other => other,
                });
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                closed = true;
                break;
            } else {
                value.push(character);
            }
        }
        if closed && !value.trim().is_empty() {
            return Some(value);
        }
    }
    None
}

struct ToolCall {
    name: String,
    input: String,
}

/// transcript 里的工具行格式为 `⚙ Name  {input}`（两个空格分隔）。
fn parse_tool_line(text: &str) -> Option<ToolCall> {
    let rest = text.strip_prefix("⚙ ")?;
    let (name, input) = rest.split_once("  ")?;
    Some(ToolCall {
        name: name.trim().to_string(),
        input: input.trim().to_string(),
    })
}

fn draft_goal(messages: &[crate::transcript::TranscriptMessage]) -> String {
    messages
        .iter()
        .find(|message| message.role == "user" && !message.text.trim().is_empty())
        .map(|message| cap_chars(&message.text, 300))
        .unwrap_or_default()
}

fn draft_progress(messages: &[crate::transcript::TranscriptMessage]) -> String {
    let user_count = messages.iter().filter(|message| message.role == "user").count();
    let tool_count = messages.iter().filter(|message| message.role == "tool").count();
    let mut sections = vec![format!(
        "用户指令 {user_count} 条；工具调用 {tool_count} 次（调用完成不代表验证通过）"
    )];
    if let Some(last) = last_assistant(messages) {
        sections.push(format!("最近助手说明（截取）：{}", cap_chars(&last.text, 400)));
    }
    sections.join("\n")
}

fn draft_remaining(messages: &[crate::transcript::TranscriptMessage]) -> String {
    let Some(last) = last_assistant(messages) else {
        return String::new();
    };
    let paragraph = last
        .text
        .split("\n\n")
        .filter(|part| !part.trim().is_empty())
        .last()
        .unwrap_or("");
    cap_chars(paragraph, 400)
}

fn last_assistant(
    messages: &[crate::transcript::TranscriptMessage],
) -> Option<&crate::transcript::TranscriptMessage> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == "assistant" && !message.text.trim().is_empty())
}

fn draft_files(messages: &[crate::transcript::TranscriptMessage]) -> String {
    let mut stats: std::collections::BTreeMap<String, (u32, u32)> = Default::default();
    for message in messages.iter().filter(|message| message.role == "tool") {
        let Some(call) = parse_tool_line(&message.text) else {
            continue;
        };
        let editing = matches!(
            call.name.as_str(),
            "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
        );
        let reading = matches!(call.name.as_str(), "Read" | "Grep" | "Glob");
        if !editing && !reading {
            continue;
        }
        let Some(path) =
            json_string_field(&call.input, &["file_path", "notebook_path", "path"])
        else {
            continue;
        };
        let entry = stats.entry(path).or_insert((0, 0));
        if editing {
            entry.0 += 1;
        } else {
            entry.1 += 1;
        }
    }
    let mut rows: Vec<(String, (u32, u32))> = stats.into_iter().collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    rows.iter()
        .take(12)
        .map(|(path, (edit, read))| {
            let mut parts = Vec::new();
            if *edit > 0 {
                parts.push(format!("修改×{edit}"));
            }
            if *read > 0 {
                parts.push(format!("读取×{read}"));
            }
            format!("- {}（{}）", cap_chars(path, 200), parts.join("，"))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

const VERIFY_MARKERS: [&str; 10] = [
    "test", "cargo ", "pytest", "vitest", "jest", "go test", "npm run", "pnpm ", "tsc", "clippy",
];

fn draft_verification(messages: &[crate::transcript::TranscriptMessage]) -> String {
    let mut commands: Vec<String> = Vec::new();
    for message in messages.iter().filter(|message| message.role == "tool") {
        let Some(call) = parse_tool_line(&message.text) else {
            continue;
        };
        if !matches!(call.name.as_str(), "Bash" | "BashOutput" | "PowerShell") {
            continue;
        }
        let Some(command) = json_string_field(&call.input, &["command", "cmd", "script"]) else {
            continue;
        };
        if !VERIFY_MARKERS.iter().any(|marker| command.contains(marker)) {
            continue;
        }
        if !commands.contains(&command) {
            commands.push(command);
        }
    }
    if commands.is_empty() {
        return String::new();
    }
    commands
        .iter()
        .rev()
        .take(5)
        .rev()
        .map(|command| format!("- `{}`（最近执行；退出结果未判定）", cap_chars(command, 160)))
        .collect::<Vec<_>>()
        .join("\n")
}

const BLOCKER_MARKERS: [&str; 8] = [
    "error[", "error:", "failed", "FAILED", "panicked", "编译失败", "测试失败", "报错",
];

fn draft_blockers(messages: &[crate::transcript::TranscriptMessage]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for message in messages {
        let candidates: Vec<&str> = if message.role == "assistant" {
            message.text.lines().collect()
        } else if message.role == "tool" {
            message
                .text
                .strip_prefix("⤷ ")
                .map(|summary| summary.lines().collect())
                .unwrap_or_default()
        } else {
            continue;
        };
        for line in candidates {
            let trimmed = line.trim();
            if trimmed.is_empty() || !BLOCKER_MARKERS.iter().any(|marker| trimmed.contains(marker))
            {
                continue;
            }
            let capped = cap_chars(trimmed, 200);
            if !lines.contains(&capped) {
                lines.push(capped);
            }
        }
    }
    if lines.is_empty() {
        return String::new();
    }
    lines
        .iter()
        .rev()
        .take(5)
        .rev()
        .map(|line| format!("- {line}"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn draft_from_messages(messages: &[crate::transcript::TranscriptMessage]) -> HandoffNotes {
    HandoffNotes {
        goal: cap_chars(&draft_goal(messages), NOTES_FIELD_CHARS),
        progress: cap_chars(&draft_progress(messages), NOTES_FIELD_CHARS),
        remaining: cap_chars(&draft_remaining(messages), NOTES_FIELD_CHARS),
        blockers: cap_chars(&draft_blockers(messages), NOTES_FIELD_CHARS),
        files: cap_chars(&draft_files(messages), NOTES_FIELD_CHARS),
        verification: cap_chars(&draft_verification(messages), NOTES_FIELD_CHARS),
        source: "规则起草（零 LLM）：基于原会话原生历史的确定性提取；工具调用与输出不代表验证通过，请逐项核对并补全。".into(),
    }
}

fn draft_from_task(
    task: &crate::multi_agent::MultiAgentTask,
    snapshot: &crate::multi_agent::MultiAgentSnapshot,
) -> HandoffNotes {
    let logs = snapshot
        .run_logs
        .iter()
        .filter(|log| log.task_id == task.id)
        .rev()
        .take(5)
        .collect::<Vec<_>>();
    let progress = if logs.is_empty() {
        format!("任务状态：{:?}", task.status)
    } else {
        logs.iter()
            .rev()
            .map(|log| format!("- [{}] {}", log.stream, cap_chars(&log.content, 160)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let files = snapshot
        .artifacts
        .iter()
        .filter(|artifact| artifact.task_id == task.id)
        .rev()
        .take(10)
        .map(|artifact| {
            format!(
                "- {}（{}；{}）",
                cap_chars(&artifact.uri, 200),
                artifact.artifact_type,
                cap_chars(&artifact.summary, 80)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let verification = task
        .verification_command
        .as_deref()
        .map(|command| format!("验收命令（未执行不代表通过）：{command}"))
        .unwrap_or_default();
    HandoffNotes {
        goal: cap_chars(&task.description, NOTES_FIELD_CHARS),
        progress: cap_chars(&progress, NOTES_FIELD_CHARS),
        remaining: cap_chars(
            &format!("验收标准：{}", task.acceptance_criteria),
            NOTES_FIELD_CHARS,
        ),
        blockers: String::new(),
        files: cap_chars(&files, NOTES_FIELD_CHARS),
        verification: cap_chars(&verification, NOTES_FIELD_CHARS),
        source: "规则起草（零 LLM）：基于项目任务描述、执行日志与交付物记录；日志内容不代表验证通过，请逐项核对并补全。".into(),
    }
}

#[tauri::command]
pub fn collaboration_handoff_autofill(
    mode: String,
    project_id: Option<String>,
    source_id: String,
    sessions: State<'_, SessionManager>,
    store: State<'_, MultiAgentStore>,
) -> Result<HandoffNotes, String> {
    let notes = match mode.as_str() {
        "session" => {
            let record = sessions
                .get(&source_id)
                .ok_or("原会话不存在".to_string())?;
            let messages = crate::session_switch_history::read(&record, sessions.inner())
                .map_err(|error| format!("读取原会话原生历史失败：{error}；可改用手动填写"))?;
            draft_from_messages(&messages)
        }
        "task" => {
            let project_id = project_id.as_deref().ok_or("请选择项目")?;
            let snapshot = store
                .snapshot(project_id)
                .map_err(|error| error.to_string())?;
            let task = snapshot
                .tasks
                .iter()
                .find(|task| task.id == source_id)
                .ok_or("任务不属于当前项目")?;
            draft_from_task(task, &snapshot)
        }
        _ => return Err("不支持的交接类型".to_string()),
    };
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[test]
    fn lifecycle_project_handoff_keeps_binding_on_refused_tree_stop_even_when_root_exited() {
        let root = std::env::temp_dir().join(format!("project-handoff-tree-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let record = sessions.create_agent(crate::sessions::NewAgentSession { avatar_id:None, project_id:"project".into(), agent_id:"agent".into(), runtime_kind:"codex".into(), agent_name:"fixture".into(), cwd:root.display().to_string() }).unwrap();
        let ptys = PtyManager::new();
        let (pty, root_pid, _) = crate::tests::lifecycle_node_pty(&ptys, &record.id, true);
        sessions.attach_runtime(&record.id, pty).unwrap();
        assert!(!ptys.is_running(pty));
        let refusal = crate::acp::TestStopFailureGuard::new(root_pid);
        assert!(stop_project_tracked_ptys(&sessions, &ptys, "project").is_err());
        assert_eq!(sessions.get(&record.id).unwrap().pty_id, Some(pty));
        drop(refusal);
        stop_project_tracked_ptys(&sessions, &ptys, "project").unwrap();
        let stopped = sessions.get(&record.id).unwrap();
        assert!(stopped.pty_id.is_none()); assert_eq!(stopped.status, crate::sessions::SessionStatus::Stopped);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skill_comparison_accepts_line_endings_but_not_stale_instructions() {
        assert_eq!(normalized("a\r\nb\r\n"), normalized("a\nb\n"));
        assert_ne!(normalized(BUILTIN_SKILL), normalized("旧版 crewvela"));
    }
    #[test]
    fn handoff_context_does_not_claim_missing_validation_passed() {
        let input = HandoffInput {
            mode: "session".into(),
            project_id: None,
            source_id: "a".into(),
            target_id: "b".into(),
            goal: "修复".into(),
            progress: String::new(),
            remaining: "验证".into(),
            blockers: String::new(),
            files: String::new(),
            verification: "未执行".into(),
        };
        let body = context(&input, "Git 不可用");
        assert!(body.contains("已执行验证及结果：未执行"));
        assert!(body.contains("不能假定原生会话已迁移"));
    }

    #[test]
    fn unfinished_handoffs_remain_recoverable_after_thirty_new_previews() {
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch("CREATE TABLE handoffs(id TEXT PRIMARY KEY, draft_json TEXT NOT NULL, updated_at INTEGER NOT NULL);").unwrap();
        connection
            .execute(
                "INSERT INTO handoffs VALUES ('old-stopped', ?1, 0)",
                [json!({"id": "old-stopped", "state": "source_stopped"}).to_string()],
            )
            .unwrap();
        for index in 1..=35 {
            connection
                .execute(
                    "INSERT INTO handoffs VALUES (?1, ?2, ?3)",
                    params![
                        format!("new-{index}"),
                        json!({"id": format!("new-{index}"), "state": "preview"}).to_string(),
                        index
                    ],
                )
                .unwrap();
        }
        let history = handoff_history(&connection).unwrap();
        assert_eq!(history.len(), 36);
        assert!(history.iter().any(|draft| draft["id"] == "old-stopped"));
    }

    #[test]
    fn terminal_reference_removes_control_sequences_and_marks_truncation_scope() {
        let reference = terminal_reference(b"\x1b[31mfailed\x1b[0m\n");
        assert!(reference.contains("failed"));
        assert!(!reference.contains('\u{1b}'));
        assert!(reference.contains("可能截断"));
    }

    fn message(role: &str, text: &str) -> crate::transcript::TranscriptMessage {
        crate::transcript::TranscriptMessage {
            role: role.into(),
            text: text.into(),
            at: None,
        }
    }

    #[test]
    fn autofill_extracts_goal_files_verification_and_remaining() {
        let messages = vec![
            message("user", "修复登录页在深色模式下的对比度问题"),
            message("assistant", "我先查看现有样式。"),
            message("tool", "⚙ Read  {\"file_path\":\"F:\\\\site\\\\login.css\"}"),
            message("tool", "⚙ Edit  {\"file_path\":\"F:\\\\site\\\\login.css\"}"),
            message("tool", "⚙ Edit  {\"file_path\":\"F:\\\\site\\\\login.css\"}"),
            message("tool", "⚙ Bash  {\"command\":\"pnpm test --filter login\"}"),
            message("tool", "⤷ 1 passed, 0 failed"),
            message("assistant", "已修复对比度。\n\n剩余：还需在浅色模式回归一次并更新截图。"),
        ];
        let notes = draft_from_messages(&messages);
        assert_eq!(notes.goal, "修复登录页在深色模式下的对比度问题");
        assert!(notes.files.contains("login.css"), "文件列表应包含修改文件：{}", notes.files);
        assert!(notes.files.contains("修改×2"));
        assert!(notes.verification.contains("pnpm test"));
        assert!(notes.verification.contains("退出结果未判定"));
        assert!(notes.remaining.contains("剩余：还需在浅色模式回归一次"));
        assert!(notes.progress.contains("用户指令 1 条"));
        assert!(!notes.blockers.is_empty(), "passed/failed 行应命中阻塞标记");
        assert!(notes.source.contains("零 LLM"));
    }

    #[test]
    fn autofill_treats_truncated_json_as_invalid_and_caps_fields() {
        let long_path = format!("Z:\\\\{}", "x".repeat(400));
        let messages = vec![
            message("tool", &format!("⚙ Read  {{\"file_path\":\"{long_path}")),
            message(
                "assistant",
                &format!("总结\n\n{}", "下一步 ".repeat(200)),
            ),
        ];
        let notes = draft_from_messages(&messages);
        assert!(notes.files.is_empty(), "未闭合的 JSON 字符串不应进入文件列表");
        assert!(notes.remaining.chars().count() <= NOTES_FIELD_CHARS + 1);
        assert!(notes.remaining.ends_with('…'));
    }

    #[test]
    fn autofill_blocks_unrelated_tools_and_dedupes_blockers() {
        let messages = vec![
            message("user", "部署服务"),
            message("tool", "⚙ WebSearch  {\"query\":\"rust deploy\"}"),
            message("tool", "⤷ error: connection refused"),
            message("tool", "⤷ error: connection refused"),
            message("tool", "⚙ Glob  {\"pattern\":\"**/*.rs\"}"),
        ];
        let notes = draft_from_messages(&messages);
        assert_eq!(notes.verification, "", "WebSearch 不应算验证命令");
        assert!(!notes.blockers.contains("connection refused\n- error: connection refused"));
        assert!(notes.blockers.contains("connection refused"));
        assert!(!notes.files.contains("pattern"), "Glob 的 pattern 不应按文件路径统计");
    }
}
