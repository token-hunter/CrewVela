use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};
use sha2::{Digest, Sha256};

use super::{
    ActivityEvent, AddDependencyInput, AgentControlMode, AgentInstruction, AgentPersona,
    AgentProfile, AgentRole, AgentRun, AgentRuntimeKind, AgentSessionRecord, AgentSessionStatus,
    AppendRuntimeEventInput, ApprovalRecord, ArtifactRecord, ClaimTaskInput,
    CollaborationMessage, CollaborationProjectActivity, CreateAgentInput, CreateProjectInput, CreateTaskInput,
    EnqueueWakeupInput, HeartbeatReceipt, HeartbeatRunInput, KnowledgeIndexReceipt,
    KnowledgeSearchResult, MemoryEntry, MultiAgentProject, MultiAgentSnapshot, MultiAgentTask,
    PermissionProfile, ProjectStatus, PublishArtifactInput, PutMemoryInput, RecoveryReceipt,
    RequestApprovalInput, ReviewDecision, ReviewVerdict, RunLogEntry, RunStatus,
    RuntimeEventRecord, SearchKnowledgeInput, SearchMemoryInput, SendCollaborationMessageInput,
    SubmitReviewDecisionInput, TaskDependencyRecord, TaskStatus, UpdateAgentInput, UpdateRunInput,
    WakeupRequest, WorkspaceMode,
};

const SCHEMA_VERSION: i64 = 9;
const MESSAGE_TASK_ACCEPTANCE: &str = "阅读并回应当前会话消息。回复使用 CREWVELA_CLI send；可调用 sessions 查找会话。不得修改项目文件、重跑原任务或提交业务审核结论。";

#[derive(Clone)]
pub struct MultiAgentStore {
    pub(super) connection: Arc<Mutex<Connection>>,
    pub(super) conversation_attachment_root: PathBuf,
    agent_lifecycle: Arc<Mutex<()>>,
}

#[path = "delegation.rs"]
pub(super) mod delegation;

#[path = "member_library.rs"]
pub(crate) mod member_library;

impl MultiAgentStore {
    pub fn new() -> Result<Self> {
        Self::open_at(crate::providers::config_dir()?.join("multi-agent.db"))
    }

    pub fn open_at(path: PathBuf) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("创建 Multi-Agent 数据目录失败: {}", parent.display()))?;
        }
        let connection = Connection::open(&path)
            .with_context(|| format!("打开 Multi-Agent 数据库失败: {}", path.display()))?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        connection.execute_batch(
            "PRAGMA foreign_keys = ON;\nPRAGMA journal_mode = WAL;\nPRAGMA synchronous = NORMAL;",
        )?;
        migrate(&connection)?;
        member_library::migrate(&connection)?;
        super::conversation::migrate(&connection)?;
        delegation::migrate(&connection)?;
        connection.execute(
            "UPDATE ma_agents SET enabled = 0 WHERE runtime_kind NOT IN ('claude_code','codex','kimi_code','pi','opencode','deepseek_harness','zcode','zcode_cli','antigravity') AND enabled != 0",
            [],
        )?;
        // Retain old approval records for audit, but do not offer actions for finished runs.
        connection.execute(
            "UPDATE ma_approvals SET status='cancelled', decided_at=?1
             WHERE status='pending' AND EXISTS (
                 SELECT 1 FROM ma_runs r WHERE r.id=ma_approvals.run_id
                 AND (r.status IN ('failed','timed_out','cancelled','interrupted')
                   OR (r.status='succeeded' AND ma_approvals.approval_type!='task_review'))
             )", [now_millis()],
        )?;
        Ok(Self {
            connection: Arc::new(Mutex::new(connection)),
            conversation_attachment_root: path.with_extension("attachments"),
            agent_lifecycle: Arc::new(Mutex::new(())),
        })
    }

    pub fn create_project(&self, input: CreateProjectInput) -> Result<MultiAgentProject> {
        self.create_project_inner(input, None, false)
    }

    pub(crate) fn create_project_with_git(
        &self, input: CreateProjectInput, git_setup: Option<super::project_git::ProjectGitPreparation>,
    ) -> Result<MultiAgentProject> {
        self.create_project_inner(input, git_setup, true)
    }

    fn create_project_inner(&self, input: CreateProjectInput,
        git_setup: Option<super::project_git::ProjectGitPreparation>, use_default_team: bool,
    ) -> Result<MultiAgentProject> {
        let name = required(&input.name, "项目名称")?;
        let goal = required(&input.goal, "项目目标")?;
        let root_path = required(&input.root_path, "项目目录")?;
        let root = Path::new(root_path);
        if let Some(setup) = git_setup { super::project_git::prepare(root, setup)?; }
        if root.exists() && !root.is_dir() {
            return Err(anyhow!("项目目录不是文件夹: {root_path}"));
        }
        if !root.exists() {
            std::fs::create_dir_all(root)
                .with_context(|| format!("创建项目目录失败: {root_path}"))?;
        }
        let canonical_root = root
            .canonicalize()
            .with_context(|| format!("解析项目目录失败: {root_path}"))?
            .display()
            .to_string();
        let default_branch = if input.default_branch.trim().is_empty() {
            "main"
        } else {
            input.default_branch.trim()
        };
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO ma_projects
             (id, name, goal, root_path, default_branch, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'active', ?6, ?6)",
            params![id, name, goal, canonical_root, default_branch, now],
        )?;
        append_event(
            &tx,
            &id,
            "project",
            &id,
            "project.created",
            "user",
            None,
            serde_json::json!({ "name": name, "goal": goal }),
            now,
        )?;
        if use_default_team && member_library::auto_apply(&tx)? {
            member_library::apply_default(&tx, &id)?;
        }
        let project = self.get_project_with(&tx, &id)?;
        tx.commit()?;
        Ok(project)
    }

    /// Worktree settings need only the task ownership and checkout location, never transcripts or logs.
    pub(crate) fn task_worktree_locations(&self) -> Result<Vec<(String, String, String, String, String, String, Option<String>)>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(
            "SELECT p.name, t.title, p.root_path, p.id, t.id, r.id,
                    json_extract(r.session_locator_json, '$.cwd')
             FROM ma_runs r JOIN ma_tasks t ON t.id = r.task_id AND t.project_id = r.project_id
             JOIN ma_projects p ON p.id = r.project_id ORDER BY r.created_at DESC"
        )?;
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?,
            row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub(crate) fn referenced_workspaces(&self) -> Result<Vec<String>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(
            "SELECT root_path FROM ma_projects
             UNION SELECT json_extract(session_locator_json, '$.cwd') FROM ma_runs
                 WHERE json_type(session_locator_json, '$.cwd') = 'text'
             UNION SELECT uri FROM ma_artifacts WHERE artifact_type IN ('workspace', 'git_commit', 'review')"
        )?;
        let paths = statement.query_map([], |row| row.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(paths)
    }

    pub fn list_projects(&self) -> Result<Vec<MultiAgentProject>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(
            "SELECT id, name, goal, root_path, default_branch, status, created_at, updated_at
             FROM ma_projects ORDER BY updated_at DESC",
        )?;
        let projects = statement
            .query_map([], project_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(projects)
    }

    pub(crate) fn rename_project(&self, project_id: &str, name: &str) -> Result<MultiAgentProject> {
        let name = required(name, "项目名称")?;
        anyhow::ensure!(!name.chars().any(char::is_control), "项目名称不能包含控制字符");
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute("UPDATE ma_projects SET name = ?2 WHERE id = ?1", params![project_id, name])?;
        let project = self.get_project_with(&tx, project_id)?;
        tx.commit()?;
        Ok(project)
    }

    pub fn list_project_activity(&self) -> Result<Vec<CollaborationProjectActivity>> {
        let connection = self.connection.lock().unwrap();
        let mut statement = connection.prepare(
            "SELECT p.id, p.name, p.goal, p.root_path, p.default_branch, p.status, p.created_at, p.updated_at,
                CASE
                    WHEN p.status = 'cancelled' THEN 'cancelled'
                    WHEN EXISTS(SELECT 1 FROM ma_approvals a WHERE a.project_id = p.id AND a.status = 'pending')
                        OR EXISTS(SELECT 1 FROM ma_conversation_rounds c WHERE c.project_id = p.id AND c.status = 'waiting_approval')
                        THEN 'pending_approval'
                    WHEN EXISTS(SELECT 1 FROM ma_conversation_rounds c WHERE c.project_id = p.id AND c.status = 'unconfirmed')
                        OR EXISTS(SELECT 1 FROM ma_runs r WHERE r.project_id = p.id AND r.status = 'waiting_input')
                        THEN 'needs_attention'
                    WHEN EXISTS(SELECT 1 FROM ma_runs r WHERE r.project_id = p.id AND r.status IN ('queued','starting','running'))
                        OR EXISTS(SELECT 1 FROM ma_conversation_rounds c WHERE c.project_id = p.id AND c.status IN ('queued','running','stopping'))
                        THEN 'running'
                    WHEN p.status = 'completed' AND EXISTS(
                        SELECT 1 FROM ma_conversation_plans plan WHERE plan.project_id = p.id
                        AND json_extract(plan.data_json, '$.managed.status') = 'completed'
                    ) THEN 'delivered'
                    ELSE p.status
                END
             FROM ma_projects p ORDER BY p.updated_at DESC, p.id",
        )?;
        let projects = statement.query_map([], |row| {
            Ok(CollaborationProjectActivity {
                project: project_from_row(row)?,
                activity: row.get(8)?,
            })
        })?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(projects)
    }

    pub fn create_agent(&self, input: CreateAgentInput) -> Result<AgentProfile> {
        input.runtime_kind.ensure_available()?;
        if !input.runtime_kind.is_collaboration_cli() {
            return Err(anyhow!("新增协作成员仅支持 CLI 执行引擎；ZCode Desktop 仅兼容已有成员，独立 Chat 不作为新成员"));
        }
        self.check_agent_avatar(input.avatar_id.as_deref())?;
        let name = required(&input.name, "Agent 名称")?;
        let connection = self.connection.lock().unwrap();
        ensure_project_exists(&connection, &input.project_id)?;
        let duplicate: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_agents WHERE project_id=?1 AND lower(name)=lower(?2))",
            params![input.project_id, name],
            |row| row.get(0),
        )?;
        if duplicate {
            return Err(anyhow!("本项目已存在同名成员，请使用不同名称"));
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        // 当前可视化工作台采用“一 Agent 一会话窗口”模型。单个 Agent 同时运行多个
        // Run 会让会话状态和人工接管语义变得不确定；并行任务应创建多个 Agent 实例。
        let _requested_max_concurrency = input.max_concurrency;
        let max_concurrency = 1;
        let capabilities = super::runtime::capabilities_for_runtime(input.runtime_kind);
        connection.execute(
            "INSERT INTO ma_agents
             (id, project_id, name, runtime_kind, role, description, persona, persona_custom, model,
              permission_profile, max_concurrency, capabilities_json, enabled, created_at, updated_at, avatar_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, ?13, ?13, ?14)",
            params![
                id,
                input.project_id,
                name,
                input.runtime_kind.as_str(),
                input.role.as_str(),
                input.description.trim(),
                input.persona.as_str(),
                sanitized_persona_custom(&input.persona, &input.persona_custom),
                input
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                input.permission_profile.as_str(),
                max_concurrency,
                serde_json::to_string(&capabilities)?,
                now,
                input.avatar_id,
            ],
        )?;
        append_event(
            &connection,
            &input.project_id,
            "agent",
            &id,
            "agent.created",
            "user",
            None,
            serde_json::json!({
                "name": name,
                "runtimeKind": input.runtime_kind,
                "role": input.role,
            }),
            now,
        )?;
        get_agent_with(&connection, &id)
    }
    /// Only model fields change; unrelated profile edits and task/session identity survive.
    /// 保存成员配置，不重建成员，也不改写已有任务、运行及会话关联。
    pub fn update_agent(&self, input: UpdateAgentInput) -> Result<AgentProfile> {
        self.check_agent_avatar(input.avatar_id.as_deref())?;
        let name = required(&input.name, "Agent 名称")?;
        let previous_profile = self.agent_profile(&input.agent_id)?;
        if input.runtime_kind != previous_profile.runtime_kind
            || (!previous_profile.enabled && input.enabled)
        {
            input.runtime_kind.ensure_available()?;
        }
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_agent_in_project(&tx, &input.agent_id, &input.project_id)?;
        let previous = get_agent_with(&tx, &input.agent_id)?;
        if previous.removed_at.is_some() {
            return Err(anyhow!("该成员已从团队移除"));
        }
        if (input.runtime_kind != previous.runtime_kind && !input.runtime_kind.is_collaboration_cli())
            || (input.enabled && !input.runtime_kind.supports_existing_collaboration()) {
            return Err(anyhow!("切换协作成员请选择 CLI 执行引擎；已有 ZCode Desktop 可保留原配置，不会自动替换"));
        }
        let duplicate: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_agents WHERE project_id=?1 AND lower(name)=lower(?2) AND id<>?3)", params![input.project_id, name, input.agent_id], |row| row.get(0))?;
        if duplicate {
            return Err(anyhow!("本项目已存在同名成员，请使用不同名称"));
        }
        if input.runtime_kind.is_retired()
            && (input.enabled || previous.runtime_kind != input.runtime_kind)
        {
            return Err(anyhow!("该 CLI 已移除，请更换 Runtime 后重试"));
        }
        let active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_runs WHERE agent_id = ?1
             AND status IN ('queued','starting','running','waiting_input','waiting_approval'))",
            [&input.agent_id],
            |row| row.get(0),
        )?;
        if active {
            return Err(anyhow!(
                "该成员有正在执行或等待处理的任务，请完成或停止任务后再编辑"
            ));
        }
        if input.runtime_kind != previous.runtime_kind {
            // 当前会话是投递投影，历史 run/session 数据不修改。新 Runtime 必须重新绑定。
            tx.execute("UPDATE ma_agent_sessions SET app_session_id=NULL, current_task_id=NULL,
                current_run_id=NULL, status='idle', control_mode='auto', last_output_at=NULL WHERE agent_id=?1", [&input.agent_id])?;
        }
        let now = now_millis();
        tx.execute(
            "UPDATE ma_agents SET name=?2, runtime_kind=?3, role=?4, description=?5,
             persona=?6, persona_custom=?7, model=?8, enabled=?9, capabilities_json=?10, updated_at=?11, avatar_id=?12 WHERE id=?1",
            params![
                input.agent_id,
                name,
                input.runtime_kind.as_str(),
                input.role.as_str(),
                input.description.trim(),
                input.persona.as_str(),
                sanitized_persona_custom(&input.persona, &input.persona_custom),
                input
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty()),
                input.enabled,
                serde_json::to_string(&super::runtime::capabilities_for_runtime(
                    input.runtime_kind
                ))?,
                now,
                input.avatar_id
            ],
        )?;
        append_event(
            &tx,
            &input.project_id,
            "agent",
            &input.agent_id,
            "agent.updated",
            "user",
            None,
            serde_json::json!({"name": name, "runtimeKind": input.runtime_kind, "role": input.role, "enabled": input.enabled}),
            now,
        )?;
        let updated = get_agent_with(&tx, &input.agent_id)?;
        if updated.name != previous.name || updated.runtime_kind != previous.runtime_kind
            || updated.role != previous.role || updated.description != previous.description
            || updated.persona != previous.persona || updated.persona_custom != previous.persona_custom
            || updated.avatar_id != previous.avatar_id || updated.model != previous.model || updated.enabled != previous.enabled {
            tx.execute("UPDATE ma_library_project_members SET follows_library=0 WHERE agent_id=?1", [&input.agent_id])?;
        }
        tx.commit()?;
        Ok(updated)
    }

    pub(crate) fn remove_agent(&self, project_id: &str, agent_id: &str, confirm: bool) -> Result<Vec<String>> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_agent_in_project(&tx, agent_id, project_id)?;
        if get_agent_with(&tx, agent_id)?.removed_at.is_some() { return Ok(Vec::new()); }
        let blockers = {
            let mut statement = tx.prepare(
                "SELECT t.title FROM ma_tasks t WHERE t.project_id=?1 AND (
                    ((t.assignee_agent_id=?2 OR t.reviewer_agent_id=?2)
                      AND t.status NOT IN ('done','cancelled')
                      AND NOT (t.status='failed' AND t.id LIKE 'cli-message-%'))
                    OR EXISTS (SELECT 1 FROM ma_runs r WHERE r.task_id=t.id AND r.agent_id=?2
                      AND (r.status IN ('queued','starting','running','waiting_input','waiting_approval')
                           OR json_extract(r.session_locator_json,'$.stopUnverified')=1))
                 ) ORDER BY t.created_at,t.id",
            )?;
            let rows = statement.query_map(params![project_id, agent_id], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<Vec<String>>>()?
        };
        let active_discussion: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_conversation_rounds r,
             json_each(r.data_json,'$.participantAgentIds') participant WHERE r.project_id=?1
               AND r.status IN ('queued','running','waiting_approval','stopping','unconfirmed')
               AND participant.value=?2)
             OR EXISTS(SELECT 1 FROM ma_conversation_messages m,
             json_each(m.data_json,'$.queuedInput.participantAgentIds') participant WHERE m.project_id=?1
               AND json_extract(m.data_json,'$.status')='queued' AND participant.value=?2)",
            params![project_id, agent_id], |row| row.get(0),
        )?;
        let mut blockers = blockers;
        if active_discussion { blockers.push("团队讨论正在进行或尚未确认停止".into()); }
        if !confirm || !blockers.is_empty() { return Ok(blockers); }

        let now = now_millis();
        tx.execute("UPDATE ma_agents SET enabled=0, removed_at=?3, updated_at=?3 WHERE project_id=?1 AND id=?2", params![project_id, agent_id, now])?;
        append_event(&tx, project_id, "agent", agent_id, "agent.removed", "user", None, serde_json::json!({}), now)?;
        tx.commit()?;
        Ok(blockers)
    }

    pub fn create_task(&self, input: CreateTaskInput) -> Result<MultiAgentTask> {
        let title = required(&input.title, "任务标题")?;
        let connection = self.connection.lock().unwrap();
        ensure_project_exists(&connection, &input.project_id)?;
        if let Some(parent_id) = input.parent_id.as_deref() {
            ensure_task_in_project(&connection, parent_id, &input.project_id)?;
        }
        for agent_id in [input.assignee_agent_id.as_deref(), input.reviewer_agent_id.as_deref()].into_iter().flatten() {
            ensure_current_agent_in_project(&connection, agent_id, &input.project_id)?;
            if get_agent_with(&connection, agent_id)?.runtime_kind == AgentRuntimeKind::Antigravity {
                return Err(anyhow!("Antigravity CLI 目前仅支持团队普通对话"));
            }
        }
        let id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        connection.execute(
            "INSERT INTO ma_tasks
             (id, project_id, parent_id, stage, title, description, acceptance_criteria,
              verification_command, status, assignee_agent_id, reviewer_agent_id, workspace_mode,
              priority, required_capabilities_json, max_attempts, retry_backoff_seconds,
              next_retry_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'ready', ?9, ?10, ?11, ?12,
                     ?13, ?14, ?15, NULL, ?16, ?16)",
            params![
                id,
                input.project_id,
                input.parent_id,
                input.stage,
                title,
                input.description.trim(),
                input.acceptance_criteria.trim(),
                input
                    .verification_command
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty()),
                input.assignee_agent_id,
                input.reviewer_agent_id,
                input.workspace_mode.as_str(),
                input.priority,
                serde_json::to_string(&normalize_capabilities(&input.required_capabilities))?,
                input.max_attempts.clamp(1, 10),
                input.retry_backoff_seconds.clamp(1, 3600) as i64,
                now,
            ],
        )?;
        append_event(
            &connection,
            &input.project_id,
            "task",
            &id,
            "task.created",
            "user",
            None,
            serde_json::json!({ "title": title, "stage": input.stage }),
            now,
        )?;
        get_task_with(&connection, &id)
    }

    /// 一次性 Runtime 的会话消息通过独立只读任务处理，消息 ID 保证重试不重复唤醒。
    pub fn enqueue_cli_message_task(
        &self,
        project_id: &str,
        agent_id: &str,
        message_id: &str,
        body: &str,
    ) -> Result<bool> {
        let message_id = required(message_id, "消息 ID")?;
        // The task ID is also used as an isolated message-workspace name.
        if !message_id.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')) {
            return Err(anyhow!("消息 ID 只能包含英文字母、数字、连字符和下划线"));
        }
        let body = required(body, "消息内容")?;
        self.project(project_id)?;
        let agent = self.agent_profile(agent_id)?;
        if agent.project_id != project_id {
            return Err(anyhow!("该 Agent 不属于目标项目"));
        }
        if !agent.enabled {
            return Ok(false);
        }
        let Some(_runtime_lease) =
            crate::runtime_preferences::acquire_if_available(agent.runtime_kind)?
        else {
            return Ok(false);
        };
        let id = format!("cli-message-{message_id}");
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_agent_in_project(&tx, agent_id, project_id)?;
        let existing = tx
            .query_row(
                "SELECT project_id, assignee_agent_id, description FROM ma_tasks WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Option<String>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?;
        if let Some((existing_project, existing_agent, existing_body)) = existing {
            if existing_project != project_id
                || existing_agent.as_deref() != Some(agent_id)
                || existing_body != body
            {
                return Err(anyhow!("消息 ID 已用于其他投递，不能重复创建任务"));
            }
            return Ok(true);
        }
        let project = self.get_project_with(&tx, project_id)?;
        let agent = get_agent_with(&tx, agent_id)?;
        if !matches!(
            project.status,
            ProjectStatus::Draft | ProjectStatus::Active | ProjectStatus::Completed
        ) || !agent.enabled
            || agent.runtime_kind.is_retired()
            || agent_is_manually_controlled(&tx, project_id, agent_id)?
        {
            return Ok(false);
        }
        let now = now_millis();
        tx.execute(
            "INSERT INTO ma_tasks
             (id, project_id, parent_id, stage, title, description, acceptance_criteria,
              verification_command, status, assignee_agent_id, reviewer_agent_id, workspace_mode,
              priority, required_capabilities_json, max_attempts, retry_backoff_seconds,
              next_retry_at, created_at, updated_at)
             VALUES (?1, ?2, NULL, 0, ?3, ?4, ?5, NULL, 'ready', ?6, NULL, 'read_only',
                     0, '[]', 1, 5, NULL, ?7, ?7)",
            params![id, project_id, format!("会话消息 · {}", agent.name), body,
                MESSAGE_TASK_ACCEPTANCE,
                agent_id, now],
        )?;
        append_event(
            &tx,
            project_id,
            "task",
            &id,
            "task.created",
            "system",
            None,
            serde_json::json!({"messageId": message_id, "targetAgentId": agent_id, "source": "cli_message"}),
            now,
        )?;
        tx.commit()?;
        Ok(true)
    }

    pub fn add_dependency(&self, input: AddDependencyInput) -> Result<MultiAgentTask> {
        if input.task_id == input.depends_on_task_id {
            return Err(anyhow!("任务不能依赖自身"));
        }
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let project_id = task_project_id(&tx, &input.task_id)?;
        ensure_task_in_project(&tx, &input.depends_on_task_id, &project_id)?;
        if dependency_reaches(&tx, &input.depends_on_task_id, &input.task_id)? {
            return Err(anyhow!("该依赖会形成循环"));
        }
        tx.execute(
            "INSERT OR IGNORE INTO ma_task_dependencies (task_id, depends_on_task_id)
             VALUES (?1, ?2)",
            params![input.task_id, input.depends_on_task_id],
        )?;
        let dependency_done: bool = tx.query_row(
            "SELECT status = 'done' FROM ma_tasks WHERE id = ?1",
            params![input.depends_on_task_id],
            |row| row.get(0),
        )?;
        if !dependency_done {
            tx.execute(
                "UPDATE ma_tasks SET status = 'blocked', updated_at = ?2
                 WHERE id = ?1 AND status IN ('draft', 'ready', 'changes_requested')",
                params![input.task_id, now_millis()],
            )?;
        }
        append_event(
            &tx,
            &project_id,
            "task",
            &input.task_id,
            "task.dependency_added",
            "user",
            None,
            serde_json::json!({ "dependsOnTaskId": input.depends_on_task_id }),
            now_millis(),
        )?;
        tx.commit()?;
        get_task_with(&connection, &input.task_id)
    }

    pub fn claim_task(&self, input: ClaimTaskInput) -> Result<AgentRun> {
        let _lifecycle = self.agent_lifecycle_guard();
        let profile = self.agent_profile(&input.agent_id)?;
        if !profile.enabled || profile.runtime_kind.is_retired() {
            return Err(anyhow!("该 Agent 已禁用或其 Runtime 已移除，不能领取任务"));
        }
        let _runtime_lease = crate::runtime_preferences::acquire(profile.runtime_kind)?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = get_task_with(&tx, &input.task_id)?;
        ensure_no_handoff(&tx, &task.project_id)?;
        let project_status = self.get_project_with(&tx, &task.project_id)?.status;
        let message_only = (task.id.starts_with("cli-message-")
            && matches!(project_status, ProjectStatus::Draft | ProjectStatus::Completed))
            || (super::conversation::is_conversation_task(&task.id)
                && project_status == ProjectStatus::Paused);
        if project_status != ProjectStatus::Active && !message_only {
            return Err(anyhow!("项目未处于运行状态，不能领取任务"));
        }
        if task.next_retry_at.is_some_and(|until| until > now_millis()) {
            return Err(anyhow!("任务仍在重试退避期内"));
        }
        ensure_agent_in_project(&tx, &input.agent_id, &task.project_id)?;
        let agent = get_agent_with(&tx, &input.agent_id)?;
        if agent.runtime_kind != profile.runtime_kind {
            return Err(anyhow!("Agent Runtime 已变化，请重新领取任务"));
        }
        if !agent.enabled || agent.runtime_kind.is_retired() {
            return Err(anyhow!("该 Agent 已禁用或其 Runtime 已移除，不能领取任务"));
        }
        let manually_controlled: i64 = tx.query_row(
            "SELECT COUNT(*) FROM ma_agent_sessions
             WHERE project_id = ?1 AND agent_id = ?2 AND control_mode = 'manual'",
            params![task.project_id, input.agent_id],
            |row| row.get(0),
        )?;
        if manually_controlled > 0 {
            return Err(anyhow!("该 Agent 正由用户接管，不能领取新任务"));
        }
        let active_agent_runs: i64 = tx.query_row(
            "SELECT COUNT(*) FROM ma_runs WHERE project_id = ?1 AND agent_id = ?2
             AND status IN ('queued','starting','running','waiting_input','waiting_approval')",
            params![task.project_id, input.agent_id],
            |row| row.get(0),
        )?;
        if active_agent_runs >= i64::from(agent.max_concurrency) {
            return Err(anyhow!("该 Agent 已达到并发上限"));
        }
        let active_message_runs: i64 = tx.query_row(
            "SELECT COUNT(*) FROM ma_runs WHERE agent_id = ?1
             AND task_id LIKE 'cli-message-%'
             AND status IN ('queued','starting','running','waiting_input','waiting_approval')",
            params![input.agent_id],
            |row| row.get(0),
        )?;
        if active_message_runs > 0 || (task.id.starts_with("cli-message-") && active_agent_runs > 0)
        {
            return Err(anyhow!("该 Agent 正在处理其他轮次，会话消息必须串行执行"));
        }
        if let Some(assignee_id) = task.assignee_agent_id.as_deref() {
            if assignee_id != input.agent_id {
                return Err(anyhow!("任务已指派给其他 Agent"));
            }
        }
        if !matches!(
            task.status,
            TaskStatus::Ready | TaskStatus::ChangesRequested
        ) {
            return Err(anyhow!("任务当前状态不可领取: {}", task.status.as_str()));
        }
        let incomplete_dependencies: i64 = tx.query_row(
            "SELECT COUNT(*)
             FROM ma_task_dependencies d
             JOIN ma_tasks dependency ON dependency.id = d.depends_on_task_id
             WHERE d.task_id = ?1 AND dependency.status != 'done'",
            params![input.task_id],
            |row| row.get(0),
        )?;
        if incomplete_dependencies > 0 {
            return Err(anyhow!("任务仍有 {incomplete_dependencies} 个未完成依赖"));
        }
        let active_runs: i64 = tx.query_row(
            "SELECT COUNT(*) FROM ma_runs
             WHERE task_id = ?1 AND status IN
             ('queued','starting','running','waiting_input','waiting_approval')",
            params![input.task_id],
            |row| row.get(0),
        )?;
        if active_runs > 0 {
            return Err(anyhow!("任务已经被活动 Run 领取"));
        }
        let attempt: u32 = tx.query_row(
            "SELECT COALESCE(MAX(attempt), 0) + 1 FROM ma_runs WHERE task_id = ?1",
            params![input.task_id],
            |row| row.get(0),
        )?;
        let now = now_millis();
        let lease_expires_at = now.saturating_add((input.lease_seconds.max(30) as i64) * 1000);
        let run_id = uuid::Uuid::new_v4().to_string();
        let updated = tx.execute(
            "UPDATE ma_tasks SET status = 'claimed', assignee_agent_id = ?2,
             next_retry_at = NULL, updated_at = ?3
             WHERE id = ?1 AND status IN ('ready','changes_requested')",
            params![input.task_id, input.agent_id, now],
        )?;
        if updated != 1 {
            return Err(anyhow!("任务已被其他 Run 领取"));
        }
        tx.execute(
            "INSERT INTO ma_runs
             (id, project_id, task_id, agent_id, attempt, status, lease_expires_at,
              last_heartbeat_at, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'starting', ?6, ?7, ?7)",
            params![
                run_id,
                task.project_id,
                input.task_id,
                input.agent_id,
                attempt,
                lease_expires_at,
                now,
            ],
        )?;
        tx.execute(
            "INSERT INTO ma_agent_sessions
             (id, project_id, agent_id, current_task_id, current_run_id, status, control_mode,
              last_output_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'starting', 'auto', NULL, ?6, ?6)
             ON CONFLICT(project_id, agent_id) DO UPDATE SET
               current_task_id = excluded.current_task_id,
               current_run_id = excluded.current_run_id,
               status = 'starting',
               updated_at = excluded.updated_at",
            params![
                uuid::Uuid::new_v4().to_string(),
                task.project_id,
                input.agent_id,
                input.task_id,
                run_id,
                now,
            ],
        )?;
        if attempt == 1 {
            tx.execute(
                "INSERT INTO ma_messages
                 (id, project_id, task_id, sender_type, sender_id, target_agent_id, body, created_at)
                 VALUES (?1, ?2, ?3, 'coordinator', NULL, ?4, ?5, ?6)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    task.project_id,
                    input.task_id,
                    input.agent_id,
                    format!("派发任务：{}", task.title),
                    now,
                ],
            )?;
        }
        append_event(
            &tx,
            &task.project_id,
            "run",
            &run_id,
            "run.claimed",
            "agent",
            Some(&input.agent_id),
            serde_json::json!({ "taskId": input.task_id, "attempt": attempt }),
            now,
        )?;
        tx.commit()?;
        get_run_with(&connection, &run_id)
    }

    pub(in crate::multi_agent) fn managed_runtime_directory(
        &self, project: &str, task: &str, agent: &str, runtime: AgentRuntimeKind,
    ) -> PathBuf {
        let key = format!("{}\0{}\0{}\0{}", project, task, agent, runtime.as_str());
        let hash = format!("{:x}", Sha256::digest(key.as_bytes()));
        self.conversation_attachment_root.with_extension("managed-runtime").join(hash)
    }

    pub(crate) fn conversation_run_locator(&self, project_id: &str, run_id: &str) -> Result<crate::session_switch_history::ConversationRunHistory> {
        use crate::session_switch_history::{ConversationRunHistory, ManagedRunHistory};
        let connection = self.connection.lock().unwrap();
        let run = get_run_with(&connection, run_id)?;
        anyhow::ensure!(run.project_id == project_id, "运行不属于该项目");
        let locator = run.session_locator.context("本次运行尚未记录原会话")?;
        // An agent may supply a run locator. Only a host receipt may select a
        // new Codex storage owner or reuse the native ID from that owner.
        let proof: Option<String> = connection.query_row(
            "SELECT payload_json FROM ma_events WHERE project_id=?1 AND entity_type='run' AND entity_id=?2
             AND actor_type='system' AND event_type='run.codex_thread' ORDER BY seq LIMIT 1",
            params![project_id, run_id], |row| row.get(0),
        ).optional()?;
        if let Some(proof) = proof {
            let proof: serde_json::Value = serde_json::from_str(&proof)?;
            anyhow::ensure!(["codexStorage", "threadId", "cwd"].iter().all(|key| proof[*key] == locator[*key])
                && (locator["adapter"] == "codex" || (locator["adapter"] == "acp" && locator["runtimeKind"] == "codex")),
                "Codex 会话归属、目录或原生 ID 与宿主回执不符");
        } else {
            anyhow::ensure!(locator.get("codexStorage").is_none(), "Codex 会话缺少宿主归属回执");
        }
        let planner = super::conversation::is_conversation_task(&run.task_id);
        let target: serde_json::Value = if planner {
            let grant: Option<String> = connection.query_row(
                "SELECT json_extract(r.data_json,'$.managedTarget') FROM ma_conversation_turns t
                 JOIN ma_conversation_rounds r ON r.id=t.round_id WHERE t.task_id=?1 AND r.project_id=?2
                 AND json_extract(t.data_json,'$.agent_id')=?3",
                params![run.task_id, project_id, run.agent_id], |row| row.get(0),
            ).context("找不到该项目、成员和群聊轮次的原生运行")?;
            let Some(grant) = grant else { return Ok(ConversationRunHistory::Chat(locator)); };
            let target: serde_json::Value = serde_json::from_str(&grant)?;
            anyhow::ensure!(target["planner"] == run.agent_id, "本轮规划成员与授权不符");
            target
        } else {
            let grant: String = connection.query_row(
                "SELECT json_extract(p.data_json,'$.managed.target') FROM ma_conversation_task_scopes s JOIN ma_conversation_plans p ON p.id=s.plan_id
                 JOIN json_each(p.data_json,'$.taskIds') t ON t.value=s.task_id
                 WHERE s.task_id=?1 AND p.project_id=?2 AND p.applied_request_id='managed-'||p.id
                 AND json_extract(p.data_json,'$.status')='applied' AND json_type(p.data_json,'$.managed.target')='object'",
                params![run.task_id, project_id], |row| row.get(0),
            ).context("该任务没有本项目已执行的托管分工授权")?;
            serde_json::from_str(&grant)?
        };
        // Runtime events can be submitted by agents. Only the host's own activity
        // receipt is authoritative for reading from a user's Claude/Codex history.
        let mut statement = connection.prepare(
            "SELECT payload_json FROM ma_events WHERE project_id=?1 AND entity_type='run' AND entity_id=?2
             AND actor_type='system' AND event_type='run.managed_history' ORDER BY seq")?;
        let rows = statement.query_map(params![project_id, run_id], |row| row.get::<_,String>(0))?;
        let mut identity = serde_json::Map::new();
        let mut phases = Vec::new();
        let mut receipt_cwd = None;
        let mut runtime = target["members"][&run.agent_id]["runtime"].as_str().map(str::to_owned);
        for row in rows {
            let proof: serde_json::Value = serde_json::from_str(&row?)?;
            anyhow::ensure!(proof["taskId"] == run.task_id && proof["agentId"] == run.agent_id, "托管宿主回执归属不匹配");
            let reported = proof["runtime"].as_str().context("托管宿主回执缺少引擎")?;
            anyhow::ensure!(runtime.as_deref().is_none_or(|prior| prior == reported), "原生引擎与托管授权不符");
            runtime = Some(reported.to_owned());
            let cwd = PathBuf::from(proof["cwd"].as_str().context("托管宿主回执缺少工作目录")?);
            anyhow::ensure!(receipt_cwd.as_ref().is_none_or(|prior| prior == &cwd), "原生工作目录与托管授权不符");
            receipt_cwd = Some(cwd);
            phases.push(proof["phase"].as_str().unwrap_or_default().to_owned());
            for (key,value) in proof["identity"].as_object().context("宿主回执没有原生身份")? {
                anyhow::ensure!(locator.get(key) == Some(value), "原生会话身份与宿主回执不符");
                identity.insert(key.to_owned(), value.clone());
            }
        }
        let kind: AgentRuntimeKind = serde_json::from_value(runtime.context("历史分工缺少原生引擎授权")?.into())?;
        anyhow::ensure!(locator["adapter"] == kind.as_str() && locator["managedTurn"] == "completed",
            "托管记录的 Runtime 或原生终态与本轮不符");
        let (cwd, identity) = if let Some(cwd) = receipt_cwd {
            anyhow::ensure!(phases == ["dispatched", "accepted", "completed"], "缺少唯一、完整的宿主原生身份回执");
            (cwd, Some(serde_json::Value::Object(identity)))
        } else {
            // Existing private Pi/Kimi/OpenCode sources must still prove the actual
            // run in their own data. Never extend this fallback to a personal profile.
            anyhow::ensure!(matches!(kind, AgentRuntimeKind::Pi | AgentRuntimeKind::KimiCode | AgentRuntimeKind::OpenCode),
                "此历史托管运行缺少独立的原生会话身份回执");
            let recorded = PathBuf::from(locator["cwd"].as_str().context("缺少托管工作目录")?);
            let path = super::external_command_path(&recorded);
            let configured = crate::providers::config_dir()?;
            let config = configured.canonicalize()?;
            let project = self.get_project_with(&connection, project_id)?;
            if planner {
                let location = Path::new("message-workspaces").join(&run.task_id);
                let project_root = Path::new(&project.root_path);
                let candidates = [configured.join(&location), config.join(&location), project_root.to_owned(), project_root.canonicalize()?];
                anyhow::ensure!(candidates.iter().any(|p| path == super::external_command_path(p)),
                    "原工作目录不属于该托管规划");
                let cwd = recorded.canonicalize()?;
                anyhow::ensure!(cwd == config.join(location) || cwd == project_root.canonicalize()?, "托管规划目录发生重定向");
                (cwd, None)
            } else {
                let location = Path::new("worktrees").join(project_id).join(&run.task_id);
                let owner = config.join(&location);
                anyhow::ensure!(path.starts_with(super::external_command_path(&configured.join(&location))) || path.starts_with(super::external_command_path(&owner)),
                    "原工作目录不在本项目的任务工作树中");
                let cwd = recorded.canonicalize()?;
                anyhow::ensure!(cwd.starts_with(&owner), "托管工作树发生重定向");
                let (repo, _) = super::executor::verify_delivery_worktree(&project, &cwd)?;
                anyhow::ensure!(repo.parent() == Some(owner.as_path()), "原工作树不属于本项目的该托管任务");
                (cwd, None)
            }
        };
        let mut pi_file = None;
        if kind == AgentRuntimeKind::Pi {
            // A retried turn can legitimately reopen an earlier run's Pi file in the same task.
            let owner = match locator["sessionPath"].as_str() {
                Some(path) => Path::new(path).file_stem().and_then(|s| s.to_str()).context("Pi 会话文件名无效")?,
                None if planner => run_id,
                None => return Err(anyhow!("本轮没有记录 Pi 原生会话文件")),
            };
            let owned: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM ma_runs WHERE id=?1 AND project_id=?2 AND task_id=?3 AND agent_id=?4)",
                params![owner, project_id, run.task_id, run.agent_id], |row| row.get(0))?;
            anyhow::ensure!(owned && (!planner || owner == run_id) && uuid::Uuid::parse_str(owner).is_ok(),
                "Pi 原生文件不属于本项目的该任务和成员");
            pi_file = Some(crate::providers::config_dir()?.join("multi-agent-sessions").join(format!("{owner}.jsonl")));
        }
        Ok(ConversationRunHistory::Managed(ManagedRunHistory {
            locator, cwd, run_id: run.id, kind, planner,
            profile: self.managed_runtime_directory(project_id, &run.task_id, &run.agent_id, kind), pi_file, identity,
        }))
    }

    pub fn update_run(&self, input: UpdateRunInput) -> Result<AgentRun> {
        self.start_run_with_storage(input, None)
    }

    pub(super) fn start_run_with_storage(&self, input: UpdateRunInput, codex: Option<crate::codex_storage::CodexStorage>) -> Result<AgentRun> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = get_run_with(&tx, &input.run_id)?;
        if current.session_locator.as_ref().is_some_and(|locator| locator["stopUnverified"] == true) {
            return Err(anyhow!("Run 停止未确认，请先确认原进程已停止并解除冻结"));
        }
        validate_run_transition(current.status, input.status)?;
        let now = now_millis();
        if let Some(storage) = codex {
            let locator = input.session_locator.as_ref().context("缺少 Codex 起始存储归属")?;
            anyhow::ensure!(input.status == RunStatus::Running && locator["adapter"] == "codex"
                && crate::codex_storage::CodexStorage::from_locator(locator)? == storage,
                "Codex 起始存储归属无效");
            append_event(&tx, &current.project_id, "run", &input.run_id, "run.codex_storage", "system", None,
                serde_json::json!({"codexStorage":storage, "cwd":locator["cwd"], "threadId":locator["threadId"]}), now)?;
        }
        let started_at = if input.status == RunStatus::Running {
            Some(now)
        } else {
            current.started_at
        };
        let finished_at = if is_terminal_run_status(input.status) {
            Some(now)
        } else {
            None
        };
        tx.execute(
            "UPDATE ma_runs
             SET status = ?2, session_locator_json = COALESCE(?3, session_locator_json),
                 output_summary = ?4, error = ?5,
                 started_at = COALESCE(started_at, ?6), finished_at = ?7
             WHERE id = ?1",
            params![
                input.run_id,
                input.status.as_str(),
                input
                    .session_locator
                    .as_ref()
                    .map(serde_json::Value::to_string),
                input.output_summary,
                input.error,
                started_at,
                finished_at,
            ],
        )?;
        let task_status = task_status_for_run(input.status);
        tx.execute(
            "UPDATE ma_tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![current.task_id, task_status.as_str(), now],
        )?;
        let session_status = session_status_for_run(input.status);
        tx.execute(
            "UPDATE ma_agent_sessions
             SET status = ?2, updated_at = ?3
             WHERE project_id = ?1 AND agent_id = ?4 AND current_run_id = ?5",
            params![
                current.project_id,
                session_status.as_str(),
                now,
                current.agent_id,
                current.id,
            ],
        )?;
        if input.status == RunStatus::Succeeded {
            unblock_ready_tasks(&tx, &current.project_id, now)?;
        }
        append_event(
            &tx,
            &current.project_id,
            "run",
            &input.run_id,
            "run.status_changed",
            "system",
            None,
            serde_json::json!({
                "from": current.status,
                "to": input.status,
                "taskId": current.task_id,
            }),
            now,
        )?;
        tx.commit()?;
        get_run_with(&connection, &input.run_id)
    }

    pub fn enqueue_wakeup(&self, input: EnqueueWakeupInput) -> Result<WakeupRequest> {
        let profile = self.agent_profile(&input.agent_id)?;
        if !profile.enabled || profile.runtime_kind.is_retired() {
            return Err(anyhow!("该 Agent 已禁用或其 Runtime 已移除，不能唤醒"));
        }
        let _runtime_lease = crate::runtime_preferences::acquire(profile.runtime_kind)?;
        let reason = required(&input.reason, "唤醒原因")?;
        let idempotency_key = required(&input.idempotency_key, "幂等键")?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_project_exists(&tx, &input.project_id)?;
        ensure_agent_in_project(&tx, &input.agent_id, &input.project_id)?;
        let agent = get_agent_with(&tx, &input.agent_id)?;
        if !agent.enabled || agent.runtime_kind.is_retired() {
            return Err(anyhow!("该 Agent 已禁用或其 Runtime 已移除，不能唤醒"));
        }
        if let Some(task_id) = input.task_id.as_deref() {
            ensure_task_in_project(&tx, task_id, &input.project_id)?;
        }
        let existing_id: Option<String> = tx
            .query_row(
                "SELECT id FROM ma_wakeups
                 WHERE idempotency_key = ?1 AND status IN ('queued','claimed')
                 ORDER BY requested_at DESC LIMIT 1",
                params![idempotency_key],
                |row| row.get(0),
            )
            .optional()?;
        let id = if let Some(id) = existing_id {
            tx.execute(
                "UPDATE ma_wakeups SET coalesced_count = coalesced_count + 1,
                 reason = ?2 WHERE id = ?1",
                params![id, reason],
            )?;
            id
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            let now = now_millis();
            tx.execute(
                "INSERT INTO ma_wakeups
                 (id, project_id, agent_id, task_id, run_id, reason, status,
                  idempotency_key, coalesced_count, requested_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued', ?7, 0, ?8)",
                params![
                    id,
                    input.project_id,
                    input.agent_id,
                    input.task_id,
                    input.run_id,
                    reason,
                    idempotency_key,
                    now,
                ],
            )?;
            append_event(
                &tx,
                &input.project_id,
                "wakeup",
                &id,
                "wakeup.queued",
                "system",
                None,
                serde_json::json!({ "agentId": input.agent_id, "taskId": input.task_id }),
                now,
            )?;
            id
        };
        tx.commit()?;
        get_wakeup_with(&connection, &id)
    }

    #[cfg(test)]
    pub fn ensure_runnable_wakeups(&self, project_id: &str) -> Result<usize> {
        self.ensure_runnable_wakeups_at(project_id, now_millis())
    }

    pub(super) fn ensure_runnable_wakeups_at(&self, project_id: &str, eligible_at: i64) -> Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_project_exists(&tx, project_id)?;
        let mut statement = tx.prepare(
            "SELECT t.id, t.assignee_agent_id, t.updated_at
             FROM ma_tasks t JOIN ma_agents a ON a.id = t.assignee_agent_id
             WHERE t.project_id = ?1 AND t.status IN ('ready','changes_requested')
               AND t.assignee_agent_id IS NOT NULL
               AND a.enabled = 1 AND a.runtime_kind IN ('claude_code','codex','kimi_code','pi','opencode','deepseek_harness','zcode','zcode_cli')
               AND (t.next_retry_at IS NULL OR t.next_retry_at <= ?2)
               AND NOT EXISTS (
                   SELECT 1 FROM ma_task_dependencies d
                   JOIN ma_tasks dependency ON dependency.id = d.depends_on_task_id
                   WHERE d.task_id = t.id AND dependency.status != 'done'
               )
               AND NOT EXISTS (
                   SELECT 1 FROM ma_wakeups w
                   WHERE w.task_id = t.id AND w.status IN ('queued','claimed')
               )",
        )?;
        let tasks = statement
            .query_map(params![project_id, eligible_at], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        let now = now_millis();
        for (task_id, agent_id, revision) in &tasks {
            let id = uuid::Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO ma_wakeups
                 (id, project_id, agent_id, task_id, run_id, reason, status,
                  idempotency_key, coalesced_count, requested_at)
                 VALUES (?1, ?2, ?3, ?4, NULL, 'task_ready', 'queued', ?5, 0, ?6)",
                params![
                    id,
                    project_id,
                    agent_id,
                    task_id,
                    format!("task:{task_id}:{revision}"),
                    now
                ],
            )?;
            append_event(
                &tx,
                project_id,
                "wakeup",
                &id,
                "wakeup.queued",
                "scheduler",
                None,
                serde_json::json!({"taskId": task_id, "agentId": agent_id}),
                now,
            )?;
        }
        tx.commit()?;
        Ok(tasks.len())
    }

    pub fn claim_task_wakeup(&self, task_id: &str) -> Result<Option<WakeupRequest>> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let wakeup_id: Option<String> = tx
            .query_row(
                "SELECT w.id FROM ma_wakeups w JOIN ma_agents a ON a.id = w.agent_id
                 WHERE w.task_id = ?1 AND w.status = 'queued' AND a.enabled = 1 AND a.runtime_kind IN ('claude_code','codex','kimi_code','pi','opencode','deepseek_harness','zcode','zcode_cli')
             ORDER BY requested_at LIMIT 1",
                params![task_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(wakeup_id) = wakeup_id else {
            return Ok(None);
        };
        let updated = tx.execute(
            "UPDATE ma_wakeups SET status = 'claimed', claimed_at = ?2 WHERE id = ?1 AND status = 'queued'",
            params![wakeup_id, now_millis()],
        )?;
        if updated != 1 {
            return Ok(None);
        }
        tx.commit()?;
        Ok(Some(get_wakeup_with(&connection, &wakeup_id)?))
    }

    pub fn finish_wakeup(&self, wakeup_id: &str, succeeded: bool) -> Result<()> {
        let connection = self.connection.lock().unwrap();
        connection.execute(
            "UPDATE ma_wakeups SET status = ?2, finished_at = ?3 WHERE id = ?1 AND status = 'claimed'",
            params![wakeup_id, if succeeded { "completed" } else { "failed" }, now_millis()],
        )?;
        Ok(())
    }

    /// Runtime 配置切换与调度领取/会话准备串行，避免新会话在切换期间被退役。
    pub(crate) fn agent_lifecycle_guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.agent_lifecycle.lock().unwrap()
    }

    pub fn agent_profile(&self, agent_id: &str) -> Result<AgentProfile> {
        get_agent_with(&self.connection.lock().unwrap(), agent_id)
    }

    pub fn snapshot(&self, project_id: &str) -> Result<MultiAgentSnapshot> {
        let connection = self.connection.lock().unwrap();
        Ok(MultiAgentSnapshot {
            project: self.get_project_with(&connection, project_id)?,
            agents: list_agents_with(&connection, project_id)?,
            tasks: list_tasks_with(&connection, project_id)?,
            runs: list_runs_with(&connection, project_id)?,
            wakeups: list_wakeups_with(&connection, project_id)?,
            events: list_events_with(&connection, project_id, 200)?,
            messages: list_messages_with(&connection, project_id)?,
            artifacts: list_artifacts_with(&connection, project_id)?,
            approvals: list_approvals_with(&connection, project_id)?,
            dependencies: list_dependencies_with(&connection, project_id)?,
            agent_sessions: list_agent_sessions_with(&connection, project_id)?,
            run_logs: list_run_logs_with(&connection, project_id, 100)?,
            instructions: list_instructions_with(&connection, project_id)?,
            runtime_events: list_runtime_events_with(&connection, project_id, None, 500)?,
            memories: list_recent_memories_with(&connection, project_id, 100)?,
            review_decisions: list_review_decisions_with(&connection, project_id)?,
        })
    }

    pub fn activity_events(
        &self,
        project_id: &str,
        after_seq: Option<i64>,
        limit: u32,
    ) -> Result<Vec<ActivityEvent>> {
        let connection = self.connection.lock().unwrap();
        ensure_project_exists(&connection, project_id)?;
        let limit = limit.clamp(1, 2_000);
        let mut statement = connection.prepare(
            "SELECT seq, project_id, entity_type, entity_id, event_type, actor_type, actor_id,
                    payload_json, created_at
             FROM ma_events
             WHERE project_id = ?1 AND seq > ?2
             ORDER BY seq LIMIT ?3",
        )?;
        let events = statement
            .query_map(
                params![project_id, after_seq.unwrap_or_default().max(0), limit],
                event_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(events)
    }

    pub fn runtime_events(
        &self,
        project_id: &str,
        after_seq: Option<i64>,
        limit: u32,
    ) -> Result<Vec<RuntimeEventRecord>> {
        let connection = self.connection.lock().unwrap();
        ensure_project_exists(&connection, project_id)?;
        list_runtime_events_with(&connection, project_id, after_seq, limit.clamp(1, 2_000))
    }

    pub fn append_runtime_event(
        &self,
        input: AppendRuntimeEventInput,
    ) -> Result<RuntimeEventRecord> {
        let event_type = required(&input.event_type, "运行事件类型")?;
        let connection = self.connection.lock().unwrap();
        let run = get_run_with(&connection, &input.run_id)?;
        let now = now_millis();
        if event_type == "agent_activity" {
            let previous = connection.query_row("SELECT seq,project_id,run_id,task_id,agent_id,event_type,payload_json,created_at FROM ma_runtime_events WHERE run_id=?1 AND event_type='agent_activity' ORDER BY seq DESC LIMIT 1", [&run.id], runtime_event_from_row).optional()?;
            if let Some(previous) = previous.filter(|event| event.payload == input.payload && now.saturating_sub(event.created_at) < 1000) { return Ok(previous); }
        }
        connection.execute(
            "INSERT INTO ma_runtime_events
             (project_id, run_id, task_id, agent_id, event_type, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                run.project_id,
                run.id,
                run.task_id,
                run.agent_id,
                event_type,
                input.payload.to_string(),
                now,
            ],
        )?;
        let event = runtime_event_by_seq(&connection, connection.last_insert_rowid())?;
        if event.task_id.starts_with("cli-message-conversation-") {
            if let Some(activity) = super::conversation_activity::signal(&event.event_type, &event.payload) {
                super::conversation_activity::persist(&connection, &event, &activity)?;
            }
        }
        drop(connection);
        self.record_conversation_runtime_event(&event)?;
        Ok(event)
    }

    pub fn heartbeat_run(&self, input: HeartbeatRunInput) -> Result<HeartbeatReceipt> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, &input.run_id)?;
        if !matches!(
            run.status,
            RunStatus::Starting
                | RunStatus::Running
                | RunStatus::WaitingInput
                | RunStatus::WaitingApproval
        ) {
            return Err(anyhow!("Run 已结束，不能续租: {}", run.status.as_str()));
        }
        let now = now_millis();
        let lease_seconds = input.lease_seconds.clamp(30, 600);
        let lease_expires_at = now.saturating_add((lease_seconds as i64) * 1_000);
        tx.execute(
            "UPDATE ma_runs SET lease_expires_at = ?2, last_heartbeat_at = ?3 WHERE id = ?1",
            params![run.id, lease_expires_at, now],
        )?;
        tx.execute(
            "UPDATE ma_agent_sessions SET updated_at = ?3
             WHERE project_id = ?1 AND agent_id = ?2 AND current_run_id = ?4",
            params![run.project_id, run.agent_id, now, run.id],
        )?;
        tx.execute(
            "INSERT INTO ma_runtime_events
             (project_id, run_id, task_id, agent_id, event_type, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, 'heartbeat', ?5, ?6)",
            params![
                run.project_id,
                run.id,
                run.task_id,
                run.agent_id,
                serde_json::json!({"leaseExpiresAt": lease_expires_at}).to_string(),
                now,
            ],
        )?;
        tx.commit()?;
        Ok(HeartbeatReceipt {
            run_id: input.run_id,
            lease_expires_at,
            heartbeat_at: now,
        })
    }

    pub fn send_collaboration_message(
        &self,
        input: SendCollaborationMessageInput,
    ) -> Result<CollaborationMessage> {
        self.send_collaboration_message_with_id(input, &uuid::Uuid::new_v4().to_string())
    }

    /// 共用持久消息与运输队列的 ID，避免相同正文的消息被错误合并。
    pub fn send_collaboration_message_with_id(
        &self,
        input: SendCollaborationMessageInput,
        message_id: &str,
    ) -> Result<CollaborationMessage> {
        let mut runtime_leases = Vec::new();
        for agent_id in input.target_agent_id.iter().chain(input.sender_id.iter()) {
            let agent = self.agent_profile(agent_id)?;
            if !agent.enabled {
                return Err(anyhow!("该 Agent 已禁用，不能发送或接收消息"));
            }
            runtime_leases.push(crate::runtime_preferences::acquire(agent.runtime_kind)?);
        }
        let message_id = required(message_id, "消息 ID")?;
        let body = required(&input.body, "消息内容")?;
        let sender_type = required(&input.sender_type, "发送者类型")?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_project_exists(&tx, &input.project_id)?;
        if let Some(task_id) = input.task_id.as_deref() {
            ensure_task_in_project(&tx, task_id, &input.project_id)?;
        }
        if let Some(sender_id) = input.sender_id.as_deref() {
            ensure_agent_in_project(&tx, sender_id, &input.project_id)?;
        }
        if let Some(target_agent_id) = input.target_agent_id.as_deref() {
            ensure_agent_in_project(&tx, target_agent_id, &input.project_id)?;
        }
        let existing = tx.query_row(
            "SELECT id, project_id, task_id, sender_type, sender_id, target_agent_id, body, created_at
             FROM ma_messages WHERE id = ?1",
            params![message_id],
            collaboration_message_from_row,
        ).optional()?;
        if let Some(existing) = existing {
            if existing.project_id == input.project_id
                && existing.task_id == input.task_id
                && existing.sender_type == sender_type
                && existing.sender_id == input.sender_id
                && existing.target_agent_id == input.target_agent_id
                && existing.body == body
            {
                return Ok(existing);
            }
            return Err(anyhow!("消息 ID 已用于不同内容，不能重复投递"));
        }
        let message = CollaborationMessage {
            id: message_id.to_string(),
            project_id: input.project_id,
            task_id: input.task_id,
            sender_type: sender_type.to_string(),
            sender_id: input.sender_id,
            target_agent_id: input.target_agent_id,
            body: body.to_string(),
            created_at: now_millis(),
        };
        tx.execute(
            "INSERT INTO ma_messages
             (id, project_id, task_id, sender_type, sender_id, target_agent_id, body, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                message.id,
                message.project_id,
                message.task_id,
                message.sender_type,
                message.sender_id,
                message.target_agent_id,
                message.body,
                message.created_at,
            ],
        )?;
        append_event(
            &tx,
            &message.project_id,
            "message",
            &message.id,
            "message.sent",
            &message.sender_type,
            message.sender_id.as_deref(),
            serde_json::json!({
                "taskId": message.task_id,
                "targetAgentId": message.target_agent_id,
            }),
            message.created_at,
        )?;
        tx.commit()?;
        Ok(message)
    }

    pub fn scoped_messages(
        &self,
        project_id: &str,
        task_id: &str,
        agent_id: &str,
        limit: u32,
    ) -> Result<Vec<CollaborationMessage>> {
        let connection = self.connection.lock().unwrap();
        ensure_task_in_project(&connection, task_id, project_id)?;
        ensure_agent_in_project(&connection, agent_id, project_id)?;
        let mut statement = connection.prepare(
            "SELECT id, project_id, task_id, sender_type, sender_id, target_agent_id, body, created_at
             FROM ma_messages
             WHERE project_id = ?1
               AND (target_agent_id = ?3 OR (
                    (task_id IS NULL OR task_id = ?2)
                    AND (target_agent_id IS NULL OR sender_id = ?3)
               ))
             ORDER BY created_at DESC LIMIT ?4",
        )?;
        let mut items = statement
            .query_map(
                params![project_id, task_id, agent_id, limit.clamp(1, 200)],
                |row| collaboration_message_from_row(row),
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        items.reverse();
        Ok(items)
    }

    pub fn publish_artifact(&self, input: PublishArtifactInput) -> Result<ArtifactRecord> {
        let artifact_type = required(&input.artifact_type, "产物类型")?;
        let uri = required(&input.uri, "产物地址")?;
        let summary = required(&input.summary, "产物摘要")?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_task_in_project(&tx, &input.task_id, &input.project_id)?;
        let run = get_run_with(&tx, &input.run_id)?;
        if run.project_id != input.project_id || run.task_id != input.task_id {
            return Err(anyhow!("产物必须属于当前 Run 的项目和任务"));
        }
        if matches!(artifact_type, "git_commit" | "workspace" | "review") {
            if let Some(target) = super::managed_delivery::task_target(&tx, &input.task_id)? {
                anyhow::ensure!(artifact_type == "review", "托管任务的 Git 交付由 CrewVela 在测试通过后登记");
                anyhow::ensure!(target.mode != super::managed_delivery::ManagedTeamMode::Pair, "最终候选提交的审查回执由 CrewVela 登记");
            }
        }
        let artifact = ArtifactRecord {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: input.project_id,
            task_id: input.task_id,
            run_id: input.run_id,
            artifact_type: artifact_type.to_string(),
            uri: uri.to_string(),
            summary: summary.to_string(),
            content_hash: input.content_hash,
            created_at: now_millis(),
        };
        tx.execute(
            "INSERT INTO ma_artifacts
             (id, project_id, task_id, run_id, artifact_type, uri, summary, content_hash, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                artifact.id,
                artifact.project_id,
                artifact.task_id,
                artifact.run_id,
                artifact.artifact_type,
                artifact.uri,
                artifact.summary,
                artifact.content_hash,
                artifact.created_at,
            ],
        )?;
        append_event(
            &tx,
            &artifact.project_id,
            "artifact",
            &artifact.id,
            "artifact.published",
            "agent",
            Some(&run.agent_id),
            serde_json::json!({"taskId": artifact.task_id, "runId": artifact.run_id}),
            artifact.created_at,
        )?;
        tx.commit()?;
        Ok(artifact)
    }

    pub(crate) fn cancel_runtime_approval(&self, approval_id: &str) -> Result<()> {
        let connection = self.connection.lock().unwrap();
        connection.execute("UPDATE ma_approvals SET status = 'cancelled', decided_at = ?2 WHERE id = ?1 AND approval_type = 'runtime_permission' AND status = 'pending'",
            params![approval_id, now_millis()])?;
        Ok(())
    }

    pub fn request_approval(&self, input: RequestApprovalInput) -> Result<ApprovalRecord> {
        let approval_type = required(&input.approval_type, "审批类型")?;
        let summary = required(&input.summary, "审批说明")?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_project_exists(&tx, &input.project_id)?;
        if let Some(task_id) = input.task_id.as_deref() {
            ensure_task_in_project(&tx, task_id, &input.project_id)?;
            if super::managed_delivery::task_target(&tx, task_id)?.is_some() {
                return Err(anyhow!("全自动托管只允许本任务范围内已授权的操作"));
            }
        }
        if let Some(run_id) = input.run_id.as_deref() {
            let run = get_run_with(&tx, run_id)?;
            if run.project_id != input.project_id || input.task_id.as_deref() != Some(&run.task_id)
            {
                return Err(anyhow!("审批请求与 Run 范围不一致"));
            }
            if !matches!(run.status, RunStatus::Starting | RunStatus::Running | RunStatus::WaitingInput | RunStatus::WaitingApproval)
                || run.session_locator.as_ref().is_some_and(|v| v["stopUnverified"] == true) {
                return Err(anyhow!("Run 未在执行或停止待确认，不能创建审批"));
            }
            let owns_task: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM ma_agent_sessions WHERE project_id=?1
                 AND agent_id=?2 AND current_task_id=?3 AND current_run_id=?4)",
                params![run.project_id, run.agent_id, run.task_id, run.id], |row| row.get(0),
            )?;
            if !owns_task { return Err(anyhow!("该 Run 已不再持有任务")); }
            if run.status == RunStatus::WaitingApproval && approval_type != "runtime_permission" {
                let existing = tx.query_row(
                    "SELECT id, project_id, task_id, run_id, approval_type, summary, status, created_at, decided_at
                     FROM ma_approvals WHERE run_id=?1 AND status='pending' AND approval_type=?2 AND summary=?3 LIMIT 1",
                    params![run.id, approval_type, summary], approval_from_row,
                ).optional()?;
                if let Some(existing) = existing { return Ok(existing); }
                // Distinct requests remain independent; approving one cannot grant the other.
            }
            if matches!(
                run.status,
                RunStatus::Starting | RunStatus::Running | RunStatus::WaitingInput
            ) {
                tx.execute(
                    "UPDATE ma_runs SET status = 'waiting_approval' WHERE id = ?1",
                    params![run.id],
                )?;
                tx.execute(
                    "UPDATE ma_tasks SET status = 'waiting_approval', updated_at = ?2 WHERE id = ?1",
                    params![run.task_id, now_millis()],
                )?;
                tx.execute(
                    "UPDATE ma_agent_sessions SET status = 'waiting', updated_at = ?3
                     WHERE project_id = ?1 AND current_run_id = ?2",
                    params![run.project_id, run.id, now_millis()],
                )?;
            }
        }
        let approval = ApprovalRecord {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: input.project_id,
            task_id: input.task_id,
            run_id: input.run_id,
            approval_type: approval_type.to_string(),
            summary: summary.to_string(),
            status: "pending".to_string(),
            created_at: now_millis(),
            decided_at: None,
        };
        tx.execute(
            "INSERT INTO ma_approvals
             (id, project_id, task_id, run_id, approval_type, summary, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)",
            params![
                approval.id,
                approval.project_id,
                approval.task_id,
                approval.run_id,
                approval.approval_type,
                approval.summary,
                approval.created_at,
            ],
        )?;
        append_event(
            &tx,
            &approval.project_id,
            "approval",
            &approval.id,
            "approval.requested",
            "agent",
            None,
            serde_json::json!({"taskId": approval.task_id, "runId": approval.run_id}),
            approval.created_at,
        )?;
        tx.commit()?;
        Ok(approval)
    }

    pub fn put_memory(&self, input: PutMemoryInput) -> Result<MemoryEntry> {
        let namespace = required(&input.namespace, "记忆命名空间")?;
        let key = required(&input.key, "记忆键")?;
        let value = required(&input.value, "记忆内容")?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_project_exists(&tx, &input.project_id)?;
        if let Some(task_id) = input.source_task_id.as_deref() {
            ensure_task_in_project(&tx, task_id, &input.project_id)?;
        }
        if let Some(run_id) = input.source_run_id.as_deref() {
            let run = get_run_with(&tx, run_id)?;
            if run.project_id != input.project_id {
                return Err(anyhow!("记忆来源 Run 不属于当前项目"));
            }
        }
        let now = now_millis();
        let id = uuid::Uuid::new_v4().to_string();
        let verified = input.source_run_id.is_none();
        tx.execute(
            "INSERT INTO ma_memory_entries
             (id, project_id, namespace, key, value, source_task_id, source_run_id,
              confidence, verified, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
             ON CONFLICT(project_id, namespace, key, source_run_id) DO UPDATE SET
               value = excluded.value,
               confidence = excluded.confidence,
               verified = CASE WHEN ma_memory_entries.verified = 1 THEN 1 ELSE excluded.verified END,
               updated_at = excluded.updated_at",
            params![
                id,
                input.project_id,
                namespace,
                key,
                value,
                input.source_task_id,
                input.source_run_id,
                input.confidence.clamp(0.0, 1.0),
                verified,
                now,
            ],
        )?;
        let entry = tx.query_row(
            "SELECT id, project_id, namespace, key, value, source_task_id, source_run_id,
                    confidence, verified, created_at, updated_at
             FROM ma_memory_entries
             WHERE project_id = ?1 AND namespace = ?2 AND key = ?3
               AND source_run_id IS ?4
             ORDER BY updated_at DESC LIMIT 1",
            params![input.project_id, namespace, key, input.source_run_id],
            memory_from_row,
        )?;
        append_event(
            &tx,
            &entry.project_id,
            "memory",
            &entry.id,
            "memory.recorded",
            if verified { "user" } else { "agent" },
            None,
            serde_json::json!({"namespace": entry.namespace, "key": entry.key, "verified": verified}),
            now,
        )?;
        tx.commit()?;
        Ok(entry)
    }

    pub fn search_memory(&self, input: SearchMemoryInput) -> Result<Vec<MemoryEntry>> {
        self.search_memory_scoped(input, None)
    }

    pub(crate) fn search_memory_for_agent(
        &self,
        input: SearchMemoryInput,
        agent_id: &str,
    ) -> Result<Vec<MemoryEntry>> {
        self.search_memory_scoped(input, Some(agent_id))
    }

    fn search_memory_scoped(&self, input: SearchMemoryInput, agent_id: Option<&str>) -> Result<Vec<MemoryEntry>> {
        let connection = self.connection.lock().unwrap();
        ensure_project_exists(&connection, &input.project_id)?;
        if let Some(agent) = agent_id {
            ensure_agent_in_project(&connection, agent, &input.project_id)?;
        }
        let query = input.query.trim();
        let pattern = if query.is_empty() {
            "%".to_string()
        } else {
            format!("%{}%", query.replace('%', "\\%").replace('_', "\\_"))
        };
        let namespace = input
            .namespace
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let mut statement = connection.prepare(
            "SELECT id, project_id, namespace, key, value, source_task_id, source_run_id,
                    confidence, verified, created_at, updated_at
             FROM ma_memory_entries
             WHERE project_id = ?1
               AND (?2 IS NULL OR namespace = ?2)
               AND (verified = 1 OR source_run_id = ?3)
               AND (?6 IS NULL OR source_task_id IS NULL OR substr(source_task_id, 1, 12) != 'cli-message-' OR EXISTS (
                   SELECT 1 FROM ma_tasks private_task
                   WHERE private_task.id = ma_memory_entries.source_task_id
                     AND private_task.project_id = ma_memory_entries.project_id
                     AND private_task.assignee_agent_id = ?6
               ))
               AND (key LIKE ?4 ESCAPE '\\' OR value LIKE ?4 ESCAPE '\\')
             ORDER BY verified DESC, confidence DESC, updated_at DESC
             LIMIT ?5",
        )?;
        let memories = statement
            .query_map(
                params![
                    input.project_id,
                    namespace,
                    input.current_run_id,
                    pattern,
                    input.limit.clamp(1, 100),
                    agent_id,
                ],
                memory_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(memories)
    }

    pub fn rebuild_knowledge_index(&self, project_id: &str) -> Result<KnowledgeIndexReceipt> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_project_exists(&tx, project_id)?;
        sync_project_knowledge_with(&tx, project_id)?;
        let mut source_counts = std::collections::BTreeMap::new();
        {
            let mut statement = tx.prepare(
                "SELECT source_type, COUNT(*)
                 FROM ma_knowledge_documents
                 WHERE project_id = ?1
                 GROUP BY source_type
                 ORDER BY source_type",
            )?;
            for row in statement.query_map(params![project_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?))
            })? {
                let (source_type, count) = row?;
                source_counts.insert(source_type, count);
            }
        }
        let document_count = source_counts.values().copied().sum();
        let indexed_at = now_millis();
        tx.commit()?;
        Ok(KnowledgeIndexReceipt {
            project_id: project_id.to_string(),
            document_count,
            source_counts,
            indexed_at,
        })
    }

    pub fn search_knowledge(
        &self,
        input: SearchKnowledgeInput,
    ) -> Result<Vec<KnowledgeSearchResult>> {
        self.search_knowledge_scoped(input, None)
    }

    /// Agent searches share the thread's visibility rules; the user-facing
    /// management search above deliberately retains the full project index.
    pub(crate) fn search_knowledge_for_agent(
        &self,
        input: SearchKnowledgeInput,
        agent_id: &str,
        current_task_id: &str,
    ) -> Result<Vec<KnowledgeSearchResult>> {
        self.search_knowledge_scoped(input, Some((agent_id, current_task_id)))
    }

    fn search_knowledge_scoped(
        &self,
        input: SearchKnowledgeInput,
        scope: Option<(&str, &str)>,
    ) -> Result<Vec<KnowledgeSearchResult>> {
        self.rebuild_knowledge_index(&input.project_id)?;
        let connection = self.connection.lock().unwrap();
        let (agent_id, current_task_id) = scope.map(|(agent, task)| (Some(agent), Some(task)))
            .unwrap_or((None, None));
        if let Some((agent, task)) = scope {
            ensure_agent_in_project(&connection, agent, &input.project_id)?;
            ensure_task_in_project(&connection, task, &input.project_id)?;
        }
        let query = input.query.trim();
        let task_id = input
            .task_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let Some(task_id) = task_id {
            ensure_task_in_project(&connection, task_id, &input.project_id)?;
        }
        let source_filter = normalize_knowledge_source_types(&input.source_types).join(",");
        let limit = input.limit.clamp(1, 100);
        let matches_source =
            "(?3 = '' OR instr(',' || ?3 || ',', ',' || d.source_type || ',') > 0)";
        // Message tasks contain the original private envelope, and their run,
        // artifact and memory index entries can quote it. Restrict all of them,
        // in addition to applying the thread predicate to conversation rows.
        let visible_to_agent = "(?7 IS NULL OR (
            (d.source_type = 'conversation' OR d.task_id IS NULL OR substr(d.task_id, 1, 12) != 'cli-message-' OR EXISTS (
                SELECT 1 FROM ma_tasks private_task
                WHERE private_task.id = d.task_id AND private_task.project_id = d.project_id
                  AND private_task.assignee_agent_id = ?7
            )) AND (d.source_type != 'conversation' OR EXISTS (
                SELECT 1 FROM ma_messages message
                WHERE message.id = d.source_id AND message.project_id = d.project_id
                  AND (message.target_agent_id = ?7 OR (
                    (message.task_id IS NULL OR message.task_id = ?8)
                    AND (message.target_agent_id IS NULL OR message.sender_id = ?7)
                  ))
            ))
        ))";
        if let Some(fts_query) = build_knowledge_fts_query(query) {
            let sql = format!(
                "SELECT d.id, d.project_id, d.source_type, d.source_id, d.task_id, d.run_id,
                        d.title,
                        snippet(ma_knowledge_fts, 1, '【', '】', '…', 36),
                        d.content, d.source_uri,
                        -bm25(ma_knowledge_fts, 4.0, 1.0) AS relevance,
                        d.created_at, d.updated_at
                 FROM ma_knowledge_fts
                 JOIN ma_knowledge_documents d ON d.rowid = ma_knowledge_fts.rowid
                 WHERE ma_knowledge_fts MATCH ?1
                   AND d.project_id = ?2
                   AND {matches_source}
                   AND {visible_to_agent}
                   AND (?4 IS NULL OR d.task_id = ?4)
                 ORDER BY relevance DESC, d.updated_at DESC, d.id ASC
                 LIMIT ?5 OFFSET ?6"
            );
            let mut statement = connection.prepare(&sql)?;
            let results = statement
                .query_map(
                    params![fts_query, input.project_id, source_filter, task_id, limit, input.offset, agent_id, current_task_id],
                    knowledge_search_result_from_row,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            return Ok(results);
        }

        let pattern = if query.is_empty() {
            "%".to_string()
        } else {
            format!("%{}%", escape_like(query))
        };
        let sql = format!(
            "SELECT d.id, d.project_id, d.source_type, d.source_id, d.task_id, d.run_id,
                    d.title, substr(d.content, 1, 360), d.content, d.source_uri,
                    0.0 AS relevance, d.created_at, d.updated_at
             FROM ma_knowledge_documents d
             WHERE d.project_id = ?1
               AND {matches_source}
               AND {visible_to_agent}
               AND (?4 IS NULL OR d.task_id = ?4)
               AND (d.title LIKE ?2 ESCAPE '\\' OR d.content LIKE ?2 ESCAPE '\\')
             ORDER BY d.updated_at DESC, d.id ASC
             LIMIT ?5 OFFSET ?6"
        );
        let mut statement = connection.prepare(&sql)?;
        let results = statement
            .query_map(
                params![input.project_id, pattern, source_filter, task_id, limit, input.offset, agent_id, current_task_id],
                knowledge_search_result_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(results)
    }

    pub fn fail_run_with_retry(
        &self,
        run_id: &str,
        error: &str,
        session_locator: Option<serde_json::Value>,
    ) -> Result<AgentRun> {
        self.fail_run(run_id, error, session_locator, true)
    }

    /// 配置类永久性错误：重试必然以同样方式失败，直接置 failed 交人工处理。
    pub fn fail_run_permanently(
        &self,
        run_id: &str,
        error: &str,
        session_locator: Option<serde_json::Value>,
    ) -> Result<AgentRun> {
        self.fail_run(run_id, error, session_locator, false)
    }

    fn fail_run(
        &self,
        run_id: &str,
        error: &str,
        session_locator: Option<serde_json::Value>,
        allow_retry: bool,
    ) -> Result<AgentRun> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let result = Self::fail_run_in(&tx, run_id, error, session_locator, allow_retry)?;
        tx.commit()?;
        Ok(result)
    }

    fn fail_run_in(
        tx: &Connection,
        run_id: &str,
        error: &str,
        session_locator: Option<serde_json::Value>,
        allow_retry: bool,
    ) -> Result<AgentRun> {
        let run = get_run_with(tx, run_id)?;
        if is_terminal_run_status(run.status) {
            return Ok(run);
        }
        if run.session_locator.as_ref().is_some_and(|locator| locator["stopUnverified"] == true) {
            return Err(anyhow!("Run 停止未确认，禁止自动结束或重试；请先确认原执行进程已停止"));
        }
        let task = get_task_with(tx, &run.task_id)?;
        let now = now_millis();
        let charged_attempt = delegation::charged_attempt(tx, &task.id, run.attempt)?;
        let should_retry = allow_retry && charged_attempt < task.max_attempts;
        let exponent = charged_attempt.saturating_sub(1).min(10);
        let backoff_seconds = task
            .retry_backoff_seconds
            .saturating_mul(1_u64 << exponent)
            .min(3_600);
        let next_retry_at = should_retry
            .then(|| now.saturating_add((backoff_seconds as i64).saturating_mul(1_000)));
        tx.execute(
            "UPDATE ma_runs SET status = 'failed', error = ?2,
                    session_locator_json = COALESCE(?3, session_locator_json), finished_at = ?4
             WHERE id = ?1",
            params![
                run.id,
                error,
                session_locator.as_ref().map(serde_json::Value::to_string),
                now,
            ],
        )?;
        tx.execute(
            "UPDATE ma_tasks SET status = ?2, next_retry_at = ?3, updated_at = ?4 WHERE id = ?1",
            params![
                task.id,
                if should_retry { "ready" } else { "failed" },
                next_retry_at,
                now,
            ],
        )?;
        tx.execute(
            "UPDATE ma_agent_sessions SET status = 'failed', updated_at = ?3
             WHERE project_id = ?1 AND current_run_id = ?2",
            params![run.project_id, run.id, now],
        )?;
        append_event(
            tx,
            &run.project_id,
            "run",
            &run.id,
            if should_retry {
                "run.retry_scheduled"
            } else {
                "run.failed"
            },
            "system",
            None,
            serde_json::json!({
                "taskId": run.task_id,
                "attempt": run.attempt,
                "maxAttempts": task.max_attempts,
                "nextRetryAt": next_retry_at,
                "error": error,
            }),
            now,
        )?;
        tx.execute(
            "INSERT INTO ma_runtime_events
             (project_id, run_id, task_id, agent_id, event_type, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, 'failed', ?5, ?6)",
            params![
                run.project_id,
                run.id,
                run.task_id,
                run.agent_id,
                serde_json::json!({"error": error, "retryScheduled": should_retry}).to_string(),
                now,
            ],
        )?;
        get_run_with(tx, run_id)
    }

    /// An unconfirmed stop must survive heartbeat expiry and application restart.
    /// Keep the active run as the execution owner until a confirmed stop is reconciled.
    pub(crate) fn mark_run_stop_unverified(&self, run_id: &str, error: &str) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, run_id)?;
        if is_terminal_run_status(run.status) { return Err(anyhow!("已结束的 Run 不能标记停止未确认")); }
        let now = now_millis();
        tx.execute("UPDATE ma_runs SET session_locator_json = json_set(COALESCE(session_locator_json, '{}'), '$.stopUnverified', json('true')), error = ?2 WHERE id = ?1", params![run_id, error])?;
        tx.execute("UPDATE ma_tasks SET status = 'blocked', next_retry_at = NULL, updated_at = ?2 WHERE id = ?1", params![run.task_id, now])?;
        append_event(&tx, &run.project_id, "run", run_id, "run.stop_unverified", "system", None,
            serde_json::json!({"error":error,"taskId":run.task_id,"manualReconciliationRequired":true}), now)?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn confirm_run_stopped(&self, run_id: &str, confirmed_stopped: bool) -> Result<AgentRun> {
        if !confirmed_stopped { return Err(anyhow!("必须先确认原执行进程已停止，才能解除停止未确认状态")); }
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, run_id)?;
        if !run.session_locator.as_ref().is_some_and(|locator| locator["stopUnverified"] == true) {
            return Err(anyhow!("该 Run 没有停止未确认标记"));
        }
        let retry_allowed = !run.task_id.starts_with("cli-message-");
        let now = now_millis();
        tx.execute("UPDATE ma_runs SET status = 'cancelled', finished_at = ?2, error = ?3, session_locator_json = json_remove(session_locator_json, '$.stopUnverified') WHERE id = ?1", params![run_id, now, if retry_allowed { "用户确认原执行进程已停止" } else { "用户确认消息进程已停止，回复结果不确定，未自动重试" }])?;
        tx.execute("UPDATE ma_tasks SET status = ?3, next_retry_at = NULL, updated_at = ?2 WHERE id = ?1", params![run.task_id, now, if retry_allowed { "ready" } else { "failed" }])?;
        tx.execute("UPDATE ma_agent_sessions SET status = 'stopped', updated_at = ?2 WHERE current_run_id = ?1", params![run_id, now])?;
        append_event(&tx, &run.project_id, "run", run_id, "run.stop_confirmed", "user", None, serde_json::json!({"taskId":run.task_id,"retryAllowed":retry_allowed}), now)?;
        tx.commit()?;
        get_run_with(&connection, run_id)
    }

    pub fn recover_expired_leases(&self, project_id: &str) -> Result<usize> {
        // Keep selection and recovery in one write transaction: a heartbeat may
        // renew the lease right up to the instant recovery obtains this lock.
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run_ids = {
            let now = now_millis();
            let mut statement = tx.prepare(
                "SELECT r.id FROM ma_runs r JOIN ma_agents a ON a.id = r.agent_id
                 WHERE r.project_id = ?1 AND a.runtime_kind IN ('claude_code','codex','kimi_code','pi','opencode','deepseek_harness','zcode','zcode_cli','antigravity')
                   AND r.status IN ('starting','running','waiting_input','waiting_approval')
                   AND r.lease_expires_at < ?2
                   AND COALESCE(json_extract(r.session_locator_json, '$.stopUnverified'), 0) != 1",
            )?;
            let run_ids = statement
                .query_map(params![project_id, now], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            run_ids
        };
        for run_id in &run_ids {
            Self::fail_run_in(&tx, run_id, "运行租约过期，已由调度器回收", None, true)?;
        }
        tx.commit()?;
        Ok(run_ids.len())
    }

    pub fn run_logs(
        &self,
        project_id: &str,
        after_seq: Option<i64>,
        limit: u32,
    ) -> Result<Vec<RunLogEntry>> {
        let connection = self.connection.lock().unwrap();
        ensure_project_exists(&connection, project_id)?;
        let limit = limit.clamp(1, 2_000);
        if let Some(after_seq) = after_seq.filter(|value| *value > 0) {
            let mut statement = connection.prepare(
                "SELECT seq, project_id, run_id, task_id, agent_id, stream, content, created_at
                 FROM ma_run_logs WHERE project_id = ?1 AND seq > ?2
                 ORDER BY seq LIMIT ?3",
            )?;
            return Ok(statement
                .query_map(params![project_id, after_seq, limit], run_log_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?);
        }
        list_run_logs_with(&connection, project_id, limit)
    }

    pub fn logs_for_run(&self, project_id: &str, run_id: &str, limit: u32) -> Result<Vec<RunLogEntry>> {
        let connection = self.connection.lock().unwrap();
        let run = get_run_with(&connection, run_id)?;
        if run.project_id != project_id {
            return Err(anyhow!("未找到该项目的执行记录"));
        }
        let mut statement = connection.prepare(
            "SELECT seq, project_id, run_id, task_id, agent_id, stream, content, created_at
             FROM ma_run_logs WHERE project_id=?1 AND run_id=?2 ORDER BY seq DESC LIMIT ?3",
        )?;
        let mut entries = statement
            .query_map(params![project_id, run_id, limit.clamp(1, 2_000)], run_log_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        entries.reverse();
        if !entries.iter().any(|entry| entry.stream == "launch_mode") {
            if let Some(mode) = connection.query_row(
                "SELECT seq,project_id,run_id,task_id,agent_id,stream,content,created_at FROM ma_run_logs
                 WHERE project_id=?1 AND run_id=?2 AND stream='launch_mode' ORDER BY seq DESC LIMIT 1",
                params![project_id, run_id], run_log_from_row,
            ).optional()? {
                entries.insert(0, mode);
            }
        }
        Ok(entries)
    }

    pub fn append_run_log(&self, run_id: &str, stream: &str, content: &str) -> Result<()> {
        let content = content.trim_end_matches(['\r', '\n']);
        if content.trim().is_empty() {
            return Ok(());
        }
        let content = if content.chars().count() > 12_000 {
            format!(
                "{}\n…（单条输出已截断）",
                content.chars().take(12_000).collect::<String>()
            )
        } else {
            content.to_string()
        };
        let connection = self.connection.lock().unwrap();
        let run = get_run_with(&connection, run_id)?;
        let now = now_millis();
        connection.execute(
            "INSERT INTO ma_run_logs
             (project_id, run_id, task_id, agent_id, stream, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                run.project_id,
                run.id,
                run.task_id,
                run.agent_id,
                stream,
                content,
                now,
            ],
        )?;
        connection.execute(
            "INSERT INTO ma_runtime_events
             (project_id, run_id, task_id, agent_id, event_type, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, 'output', ?5, ?6)",
            params![
                run.project_id,
                run.id,
                run.task_id,
                run.agent_id,
                serde_json::json!({"stream": stream, "content": content}).to_string(),
                now,
            ],
        )?;
        let sequence = connection.last_insert_rowid();
        if sequence % 500 == 0 {
            connection.execute(
                "DELETE FROM ma_run_logs WHERE project_id = ?1 AND seq NOT IN (
                   SELECT seq FROM ma_run_logs WHERE project_id = ?1 ORDER BY seq DESC LIMIT 10000
                 )",
                params![run.project_id],
            )?;
            connection.execute(
                "DELETE FROM ma_runtime_events WHERE project_id = ?1 AND seq NOT IN (
                   SELECT seq FROM ma_runtime_events
                   WHERE project_id = ?1 ORDER BY seq DESC LIMIT 20000
                 )",
                params![run.project_id],
            )?;
        }
        connection.execute(
            "UPDATE ma_agent_sessions SET last_output_at = ?3, updated_at = ?3
             WHERE project_id = ?1 AND agent_id = ?2",
            params![run.project_id, run.agent_id, now],
        )?;
        connection.execute(
            "UPDATE ma_runs SET last_heartbeat_at = ?2,
                    lease_expires_at = MAX(lease_expires_at, ?3)
             WHERE id = ?1 AND status IN ('starting','running','waiting_input','waiting_approval')",
            params![run.id, now, now.saturating_add(120_000)],
        )?;
        Ok(())
    }

    pub fn link_agent_app_session(
        &self,
        project_id: &str,
        agent_id: &str,
        app_session_id: &str,
    ) -> Result<()> {
        let connection = self.connection.lock().unwrap();
        let now = now_millis();
        connection.execute(
            "INSERT INTO ma_agent_sessions
             (id, project_id, agent_id, current_task_id, current_run_id, app_session_id, status,
              control_mode, last_output_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, NULL, ?4, 'idle', 'auto', NULL, ?5, ?5)
             ON CONFLICT(project_id, agent_id) DO UPDATE SET
               app_session_id = excluded.app_session_id,
               updated_at = excluded.updated_at",
            params![
                uuid::Uuid::new_v4().to_string(),
                project_id,
                agent_id,
                app_session_id,
                now,
            ],
        )?;
        Ok(())
    }

    pub fn set_agent_control(
        &self,
        project_id: &str,
        agent_id: &str,
        mode: AgentControlMode,
        instruction: Option<&str>,
    ) -> Result<bool> {
        let _runtime_lease =
            crate::runtime_preferences::acquire(self.agent_profile(agent_id)?.runtime_kind)?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_project_exists(&tx, project_id)?;
        ensure_agent_in_project(&tx, agent_id, project_id)?;
        let now = now_millis();
        tx.execute(
            "INSERT INTO ma_agent_sessions
             (id, project_id, agent_id, current_task_id, current_run_id, status, control_mode,
              last_output_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, NULL, NULL, 'idle', ?4, NULL, ?5, ?5)
             ON CONFLICT(project_id, agent_id) DO UPDATE SET
               control_mode = excluded.control_mode,
               updated_at = excluded.updated_at",
            params![
                uuid::Uuid::new_v4().to_string(),
                project_id,
                agent_id,
                mode.as_str(),
                now,
            ],
        )?;
        let instruction = instruction.map(str::trim).filter(|value| !value.is_empty());
        if let Some(body) = instruction {
            tx.execute(
                "INSERT INTO ma_agent_instructions
                 (id, project_id, agent_id, task_id, body, status, created_at)
                 VALUES (?1, ?2, ?3, NULL, ?4, 'queued', ?5)",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    project_id,
                    agent_id,
                    body,
                    now
                ],
            )?;
            tx.execute(
                "INSERT INTO ma_messages
                 (id, project_id, task_id, sender_type, sender_id, target_agent_id, body, created_at)
                 VALUES (?1, ?2, NULL, 'user', NULL, ?3, ?4, ?5)",
                params![uuid::Uuid::new_v4().to_string(), project_id, agent_id, body, now],
            )?;
        }
        append_event(
            &tx,
            project_id,
            "agent",
            agent_id,
            "agent.control_changed",
            "user",
            None,
            serde_json::json!({"controlMode": mode, "instructionQueued": instruction.is_some()}),
            now,
        )?;
        tx.commit()?;
        Ok(instruction.is_some())
    }

    pub fn claim_agent_instructions(
        &self,
        project_id: &str,
        agent_id: &str,
        task_id: &str,
    ) -> Result<Vec<String>> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement = tx.prepare(
            "SELECT id, body FROM ma_agent_instructions
             WHERE project_id = ?1 AND agent_id = ?2 AND status = 'queued'
               AND (task_id IS NULL OR task_id = ?3)
             ORDER BY created_at",
        )?;
        let rows = statement
            .query_map(params![project_id, agent_id, task_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        let now = now_millis();
        for (id, _) in &rows {
            tx.execute(
                "UPDATE ma_agent_instructions
                 SET status = 'delivered', task_id = ?2, delivered_at = ?3 WHERE id = ?1",
                params![id, task_id, now],
            )?;
        }
        tx.commit()?;
        Ok(rows.into_iter().map(|(_, body)| body).collect())
    }

    pub fn runnable_tasks(&self, project_id: &str) -> Result<Vec<(MultiAgentTask, AgentProfile)>> {
        self.runnable_tasks_at(project_id, now_millis())
    }

    pub(super) fn runnable_tasks_at(&self, project_id: &str, eligible_at: i64) -> Result<Vec<(MultiAgentTask, AgentProfile)>> {
        let disabled = crate::runtime_preferences::get()?.disabled_runtimes;
        let connection = self.connection.lock().unwrap();
        ensure_project_exists(&connection, project_id)?;
        let mut statement = connection.prepare(
            "SELECT t.id, t.project_id, t.parent_id, t.stage, t.title, t.description,
                    t.acceptance_criteria, t.verification_command, t.status, t.assignee_agent_id,
                    t.reviewer_agent_id, t.workspace_mode, t.priority, t.created_at, t.updated_at,
                    t.required_capabilities_json, t.max_attempts, t.retry_backoff_seconds,
                    t.next_retry_at,
                    a.id, a.project_id, a.name, a.runtime_kind, a.role, a.description,
                    a.model, a.permission_profile, a.max_concurrency, a.enabled,
                    a.created_at, a.updated_at, a.capabilities_json, a.avatar_id,
                    a.persona, a.persona_custom, a.removed_at
             FROM ma_tasks t
             JOIN ma_agents a ON a.id = t.assignee_agent_id
             WHERE t.project_id = ?1 AND t.status IN ('ready','changes_requested') AND a.enabled = 1
               AND NOT EXISTS (SELECT 1 FROM ma_conversation_turns ct WHERE ct.task_id=t.id)
               AND NOT EXISTS (
                   SELECT 1 FROM ma_runs message_run WHERE message_run.agent_id = a.id
                     AND message_run.task_id LIKE 'cli-message-%'
                     AND message_run.status IN ('queued','starting','running','waiting_input','waiting_approval')
               )
               AND (t.id NOT LIKE 'cli-message-%' OR NOT EXISTS (
                   SELECT 1 FROM ma_runs other_run WHERE other_run.agent_id = a.id
                     AND other_run.status IN ('queued','starting','running','waiting_input','waiting_approval')
               ))
               AND (t.next_retry_at IS NULL OR t.next_retry_at <= ?2)
               AND NOT EXISTS (
                   SELECT 1 FROM ma_agent_sessions s
                   WHERE s.project_id = t.project_id AND s.agent_id = a.id
                     AND s.control_mode = 'manual'
               )
               AND NOT EXISTS (
                   SELECT 1 FROM ma_task_dependencies d
                   JOIN ma_tasks dependency ON dependency.id = d.depends_on_task_id
                   WHERE d.task_id = t.id AND dependency.status != 'done'
               )
               AND (
                   SELECT COUNT(*) FROM ma_runs r
                   WHERE r.agent_id = a.id AND r.status IN
                   ('queued','starting','running','waiting_input','waiting_approval')
               ) < a.max_concurrency
             ORDER BY t.stage, t.priority DESC, t.created_at",
        )?;
        let rows = statement.query_map(params![project_id, eligible_at], |row| {
            Ok((task_from_row(row)?, agent_from_row_offset(row, 19)?))
        })?;
        let rows = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows
            .into_iter()
            .filter(|(task, agent)| {
                !disabled.contains(&agent.runtime_kind) && agent_satisfies_task(agent, task)
            })
            .collect())
    }

    pub fn route_runnable_tasks(&self, project_id: &str) -> Result<usize> {
        let disabled = crate::runtime_preferences::get()?.disabled_runtimes;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_project_exists(&tx, project_id)?;
        let tasks = list_tasks_with(&tx, project_id)?
            .into_iter()
            .filter(|task| {
                matches!(
                    task.status,
                    TaskStatus::Ready | TaskStatus::ChangesRequested
                ) && task
                    .next_retry_at
                    .map(|retry_at| retry_at <= now_millis())
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        let agents = list_agents_with(&tx, project_id)?;
        let now = now_millis();
        let mut routed = 0;
        for task in tasks {
            // 定向消息必须由原收件人处理，禁用或人工接管时不能改派给其他 Agent。
            if task.id.starts_with("cli-message-") {
                continue;
            }
            let dependencies_incomplete: i64 = tx.query_row(
                "SELECT COUNT(*) FROM ma_task_dependencies d
                 JOIN ma_tasks dependency ON dependency.id = d.depends_on_task_id
                 WHERE d.task_id = ?1 AND dependency.status != 'done'",
                params![task.id],
                |row| row.get(0),
            )?;
            if dependencies_incomplete > 0 {
                continue;
            }
            let current = task
                .assignee_agent_id
                .as_deref()
                .and_then(|id| agents.iter().find(|agent| agent.id == id));
            if current.is_some_and(|agent| {
                agent.runtime_kind.is_retired()
                    || disabled.contains(&agent.runtime_kind)
            }) {
                continue;
            }
            if current.is_some_and(|agent| {
                agent.enabled
                    && agent_satisfies_task(agent, &task)
                    && !agent_is_manually_controlled(&tx, project_id, &agent.id).unwrap_or(true)
            }) {
                continue;
            }
            let desired_role = current.map(|agent| agent.role).unwrap_or(AgentRole::Worker);
            let mut candidates = agents
                .iter()
                .filter(|agent| {
                    agent.enabled
                        && !disabled.contains(&agent.runtime_kind)
                        && agent.role == desired_role
                        && agent_satisfies_task(agent, &task)
                        && !agent_is_manually_controlled(&tx, project_id, &agent.id).unwrap_or(true)
                })
                .map(|agent| {
                    let active: i64 = tx
                        .query_row(
                            "SELECT COUNT(*) FROM ma_runs
                             WHERE agent_id = ?1 AND status IN
                             ('queued','starting','running','waiting_input','waiting_approval')",
                            params![agent.id],
                            |row| row.get(0),
                        )
                        .unwrap_or(i64::MAX / 4);
                    let (total, successes): (i64, i64) = tx
                        .query_row(
                            "SELECT COUNT(*), SUM(CASE WHEN status = 'succeeded' THEN 1 ELSE 0 END)
                             FROM ma_runs WHERE agent_id = ?1",
                            params![agent.id],
                            |row| Ok((row.get(0)?, row.get::<_, Option<i64>>(1)?.unwrap_or(0))),
                        )
                        .unwrap_or((0, 0));
                    let success_score = if total == 0 {
                        0
                    } else {
                        successes.saturating_mul(100) / total
                    };
                    (agent, success_score - active.saturating_mul(25))
                })
                .collect::<Vec<_>>();
            candidates.sort_by(|(left_agent, left_score), (right_agent, right_score)| {
                right_score
                    .cmp(left_score)
                    .then_with(|| left_agent.created_at.cmp(&right_agent.created_at))
            });
            let Some((selected, _)) = candidates.first() else {
                continue;
            };
            tx.execute(
                "UPDATE ma_tasks SET assignee_agent_id = ?2, updated_at = ?3 WHERE id = ?1",
                params![task.id, selected.id, now],
            )?;
            tx.execute(
                "UPDATE ma_wakeups SET status = 'cancelled', finished_at = ?2
                 WHERE task_id = ?1 AND status = 'queued'",
                params![task.id, now],
            )?;
            append_event(
                &tx,
                project_id,
                "task",
                &task.id,
                "task.capability_routed",
                "scheduler",
                None,
                serde_json::json!({
                    "fromAgentId": task.assignee_agent_id,
                    "toAgentId": selected.id,
                    "requiredCapabilities": task.required_capabilities,
                }),
                now,
            )?;
            routed += 1;
        }
        tx.commit()?;
        Ok(routed)
    }

    pub fn project(&self, project_id: &str) -> Result<MultiAgentProject> {
        let connection = self.connection.lock().unwrap();
        self.get_project_with(&connection, project_id)
    }

    pub fn dependency_artifacts(&self, task_id: &str) -> Result<Vec<ArtifactRecord>> {
        let connection = self.connection.lock().unwrap();
        let managed = super::managed_delivery::task_target(&connection, task_id)?.is_some();
        let mut statement = connection.prepare(
            "WITH RECURSIVE dependency_walk(task_id, depth) AS (
                 SELECT depends_on_task_id, 1 FROM ma_task_dependencies WHERE task_id = ?1
                 UNION ALL
                 SELECT d.depends_on_task_id, parent.depth + 1
                 FROM ma_task_dependencies d
                 JOIN dependency_walk parent ON d.task_id = parent.task_id
             ), dependency_tasks(task_id, depth) AS (
                 SELECT task_id, MAX(depth) FROM dependency_walk GROUP BY task_id
             )
             SELECT DISTINCT a.id, a.project_id, a.task_id, a.run_id,
                    CASE
                      WHEN a.artifact_type = 'workspace' AND task.workspace_mode = 'worktree' THEN 'git_commit'
                      WHEN a.artifact_type = 'workspace' AND task.workspace_mode = 'read_only' THEN 'review'
                      WHEN a.artifact_type = 'workspace' AND task.workspace_mode = 'shared_write' THEN 'shared_workspace'
                      ELSE a.artifact_type
                    END,
                    a.uri,
                    a.summary, a.content_hash, a.created_at
             FROM dependency_tasks d
             JOIN ma_artifacts a ON a.task_id = d.task_id
             JOIN ma_tasks task ON task.id = a.task_id
             WHERE NOT ?2 OR (a.artifact_type <> 'workspace' AND (
                 a.artifact_type <> 'git_commit' OR a.run_id = (
                     SELECT id FROM ma_runs WHERE task_id = task.id AND status = 'succeeded'
                     ORDER BY attempt DESC, created_at DESC LIMIT 1
                 )
             ))
             ORDER BY d.depth DESC, a.created_at, a.id",
        )?;
        let items = statement
            .query_map(params![task_id, managed], |row| {
                Ok(ArtifactRecord {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    task_id: row.get(2)?,
                    run_id: row.get(3)?,
                    artifact_type: row.get(4)?,
                    uri: row.get(5)?,
                    summary: row.get(6)?,
                    content_hash: row.get(7)?,
                    created_at: row.get(8)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(items)
    }

    pub fn latest_session_locator(&self, task_id: &str) -> Result<Option<serde_json::Value>> {
        let connection = self.connection.lock().unwrap();
        let raw: Option<String> = connection
            .query_row(
                "SELECT session_locator_json FROM ma_runs
             WHERE task_id = ?1 AND session_locator_json IS NOT NULL
             ORDER BY attempt DESC LIMIT 1",
                params![task_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        raw.map(|value| serde_json::from_str(&value).context("解析历史 Session locator 失败"))
            .transpose()
    }

    pub(super) fn codex_locator_for_retry(&self, task_id: &str) -> Result<Option<serde_json::Value>> {
        let db = self.connection.lock().unwrap();
        let latest: Option<(String,String)> = db.query_row(
            "SELECT id,session_locator_json FROM ma_runs WHERE task_id=?1 AND session_locator_json IS NOT NULL ORDER BY attempt DESC LIMIT 1",
            [task_id], |row| Ok((row.get(0)?,row.get(1)?))).optional()?;
        let Some((run, raw)) = latest else { return Ok(None); };
        let locator: serde_json::Value = serde_json::from_str(&raw)?;
        let mut native = None;
        let mut owner = None;
        let mut query = db.prepare("SELECT event_type,payload_json FROM ma_events WHERE entity_type='run' AND entity_id=?1
            AND actor_type='system' AND event_type IN ('run.codex_storage','run.codex_thread','run.managed_history') ORDER BY seq")?;
        for row in query.query_map([run], |row| Ok((row.get::<_,String>(0)?,row.get::<_,String>(1)?)))? {
            let (kind, raw) = row?;
            let proof: serde_json::Value = serde_json::from_str(&raw)?;
            if kind == "run.managed_history" {
                if let Some(id) = proof["identity"]["threadId"].as_str() { native = Some(id.to_owned()); }
            } else {
                anyhow::ensure!(proof["codexStorage"] == locator["codexStorage"] && proof["cwd"] == locator["cwd"],
                    "Codex 续跑归属或目录与宿主回执不符");
                owner = Some(crate::codex_storage::CodexStorage::from_locator(&proof)?);
                if let Some(id) = proof["threadId"].as_str() { native = Some(id.to_owned()); }
            }
        }
        if owner.is_some() || locator.get("codexStorage").is_some() {
            let recorded = ["threadId", "thread_id", "sessionId"].iter().filter_map(|key| locator[*key].as_str());
            anyhow::ensure!(owner.is_some() && recorded.clone().all(|id| native.as_deref() == Some(id))
                && native.as_ref().is_none_or(|_| recorded.count() != 0), "Codex 续跑缺少原生身份的宿主回执");
        }
        Ok(Some(locator))
    }

    pub fn record_message(
        &self,
        project_id: &str,
        sender_type: &str,
        body: &str,
    ) -> Result<CollaborationMessage> {
        let body = required(body, "消息内容")?;
        let connection = self.connection.lock().unwrap();
        ensure_project_exists(&connection, project_id)?;
        let message = CollaborationMessage {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.to_string(),
            task_id: None,
            sender_type: sender_type.to_string(),
            sender_id: None,
            target_agent_id: None,
            body: body.to_string(),
            created_at: now_millis(),
        };
        connection.execute(
            "INSERT INTO ma_messages
             (id, project_id, task_id, sender_type, sender_id, target_agent_id, body, created_at)
             VALUES (?1, ?2, NULL, ?3, NULL, NULL, ?4, ?5)",
            params![
                message.id,
                message.project_id,
                message.sender_type,
                message.body,
                message.created_at
            ],
        )?;
        Ok(message)
    }

    pub fn clear_coordinator_messages(&self, project_id: &str) -> Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_project_exists(&tx, project_id)?;
        let deleted = tx.execute(
            "DELETE FROM ma_messages
             WHERE project_id = ?1 AND sender_type IN ('user', 'coordinator')",
            params![project_id],
        )?;
        append_event(
            &tx,
            project_id,
            "project",
            project_id,
            "conversation.cleared",
            "user",
            None,
            serde_json::json!({"deletedMessages": deleted}),
            now_millis(),
        )?;
        tx.commit()?;
        Ok(deleted)
    }

    pub fn set_project_status(&self, project_id: &str, status: ProjectStatus) -> Result<()> {
        let connection = self.connection.lock().unwrap();
        Self::set_project_status_in(&connection, project_id, status)
    }

    pub(super) fn set_project_status_in(connection: &Connection, project_id: &str, status: ProjectStatus) -> Result<()> {
        ensure_project_exists(connection, project_id)?;
        if status == ProjectStatus::Active {
            ensure_no_handoff(connection, project_id)?;
        }
        let status_value = match status {
            ProjectStatus::Draft => "draft",
            ProjectStatus::Active => "active",
            ProjectStatus::Paused => "paused",
            ProjectStatus::Completed => "completed",
            ProjectStatus::Cancelled => "cancelled",
        };
        let now = now_millis();
        connection.execute(
            "UPDATE ma_projects SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![project_id, status_value, now],
        )?;
        append_event(
            connection,
            project_id,
            "project",
            project_id,
            "project.status_changed",
            "user",
            None,
            serde_json::json!({"status": status_value}),
            now,
        )?;
        Ok(())
    }

    pub fn submit_review_decision(
        &self,
        input: SubmitReviewDecisionInput,
    ) -> Result<ReviewDecision> {
        let summary = required(&input.summary, "审核结论")?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, &input.review_run_id)?;
        if !matches!(
            run.status,
            RunStatus::Starting
                | RunStatus::Running
                | RunStatus::WaitingInput
                | RunStatus::WaitingApproval
        ) {
            return Err(anyhow!("审核 Run 已结束，不能再提交结论"));
        }
        let reviewer = get_agent_with(&tx, &run.agent_id)?;
        if reviewer.role != AgentRole::Reviewer {
            return Err(anyhow!("只有 Reviewer Agent 可以提交审核结论"));
        }
        let target = get_task_with(&tx, &input.target_task_id)?;
        if target.project_id != run.project_id {
            return Err(anyhow!("被审核任务不属于当前项目"));
        }
        let watchdog_target = super::governance::watchdog_target(&tx, &run.task_id)?;
        if let Some(expected_target) = watchdog_target {
            if expected_target != target.id {
                return Err(anyhow!("语义复核只能提交调度器指定的目标任务"));
            }
        } else {
            let dependency_count: u32 = tx.query_row(
                "SELECT COUNT(*) FROM ma_task_dependencies WHERE task_id = ?1",
                params![run.task_id],
                |row| row.get(0),
            )?;
            if dependency_count != 1 {
                return Err(anyhow!("每个 Reviewer 任务必须且只能有一个直接被审核任务"));
            }
            let is_direct_dependency: bool = tx.query_row(
                "SELECT EXISTS(
                     SELECT 1 FROM ma_task_dependencies
                     WHERE task_id = ?1 AND depends_on_task_id = ?2
                 )",
                params![run.task_id, target.id],
                |row| row.get(0),
            )?;
            if !is_direct_dependency {
                return Err(anyhow!("Reviewer 只能审核当前审核任务的直接依赖"));
            }
            if target.status != TaskStatus::Done {
                return Err(anyhow!("被审核任务尚未完成，不能提交审核结论"));
            }
        }
        let existing = tx
            .query_row(
                "SELECT id, project_id, review_task_id, review_run_id, reviewer_agent_id,
                        target_task_id, verdict, summary, created_at, applied_at
                 FROM ma_review_decisions WHERE review_run_id = ?1",
                params![run.id],
                review_decision_from_row,
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing.target_task_id == target.id
                && existing.verdict == input.verdict
                && existing.summary == summary
            {
                return Ok(existing);
            }
            return Err(anyhow!("当前审核 Run 已提交过不同的结论"));
        }
        let decision = ReviewDecision {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: run.project_id.clone(),
            review_task_id: run.task_id.clone(),
            review_run_id: run.id.clone(),
            reviewer_agent_id: run.agent_id.clone(),
            target_task_id: target.id,
            verdict: input.verdict,
            summary: summary.to_string(),
            created_at: now_millis(),
            applied_at: None,
        };
        tx.execute(
            "INSERT INTO ma_review_decisions
             (id, project_id, review_task_id, review_run_id, reviewer_agent_id,
              target_task_id, verdict, summary, created_at, applied_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL)",
            params![
                decision.id,
                decision.project_id,
                decision.review_task_id,
                decision.review_run_id,
                decision.reviewer_agent_id,
                decision.target_task_id,
                decision.verdict.as_str(),
                decision.summary,
                decision.created_at,
            ],
        )?;
        append_event(
            &tx,
            &decision.project_id,
            "review",
            &decision.id,
            "review.submitted",
            "agent",
            Some(&decision.reviewer_agent_id),
            serde_json::json!({
                "reviewTaskId": decision.review_task_id,
                "targetTaskId": decision.target_task_id,
                "verdict": decision.verdict,
            }),
            decision.created_at,
        )?;
        tx.commit()?;
        Ok(decision)
    }

    pub fn finalize_successful_run(
        &self,
        run_id: &str,
        workspace_uri: &str,
        summary: &str,
        acceptance_verified: bool,
        content_hash: Option<&str>,
        session_locator: Option<&serde_json::Value>,
    ) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, run_id)?;
        validate_run_transition(run.status, RunStatus::Succeeded)?;
        let task = get_task_with(&tx, &run.task_id)?;
        let producer = get_agent_with(&tx, &run.agent_id)?;
        let managed_target = super::managed_delivery::task_target(&tx, &run.task_id)?;
        if let Some(target) = &managed_target {
            if task.workspace_mode == WorkspaceMode::Worktree {
                anyhow::ensure!(acceptance_verified && content_hash.is_some(), "托管交付必须先通过验收测试并保存 Git 提交");
                if producer.role == AgentRole::Integrator {
                    anyhow::ensure!(Some(super::executor::git_head(Path::new(&target.root))?.as_str()) == content_hash,
                        "托管集成提交尚未合入授权交付分支");
                }
            }
        }
        let message_turn = task.id.starts_with("cli-message-");
        let review_decision = if producer.role == AgentRole::Reviewer && !message_turn {
            Some(
                tx.query_row(
                    "SELECT id, project_id, review_task_id, review_run_id, reviewer_agent_id,
                            target_task_id, verdict, summary, created_at, applied_at
                     FROM ma_review_decisions WHERE review_run_id = ?1",
                    params![run.id],
                    review_decision_from_row,
                )
                .optional()?
                .ok_or_else(|| anyhow!("Reviewer 必须通过 cc_review_submit 提交审核结论"))?,
            )
        } else {
            None
        };
        let pair_review = managed_target.as_ref().is_some_and(|target| target.mode == super::managed_delivery::ManagedTeamMode::Pair)
            && task.workspace_mode == WorkspaceMode::ReadOnly;
        if pair_review {
            let decision = review_decision.as_ref().context("最终只读审查缺少结论")?;
            let delivered: String = tx.query_row(
                "SELECT a.content_hash FROM ma_artifacts a JOIN ma_runs r ON r.id=a.run_id
                 WHERE a.task_id=?1 AND a.artifact_type='git_commit' AND r.status='succeeded'
                 ORDER BY r.attempt DESC LIMIT 1", [&decision.target_task_id], |r| r.get(0))?;
            anyhow::ensure!(content_hash == Some(delivered.as_str())
                && super::executor::git_head(Path::new(workspace_uri))? == delivered,
                "最终审查工作树未检出当前开发任务的候选提交");
        }
        let now = now_millis();
        tx.execute(
            "UPDATE ma_runs
             SET status = 'succeeded', session_locator_json = COALESCE(?2, session_locator_json),
                 output_summary = ?3, error = NULL, started_at = COALESCE(started_at, ?4),
                 finished_at = ?4
             WHERE id = ?1",
            params![
                run.id,
                session_locator.map(serde_json::Value::to_string),
                summary,
                now,
            ],
        )?;
        tx.execute(
            "UPDATE ma_agent_sessions SET status = 'completed', updated_at = ?2
             WHERE project_id = ?1 AND agent_id = ?3 AND current_run_id = ?4",
            params![run.project_id, now, run.agent_id, run.id],
        )?;
        append_event(
            &tx,
            &run.project_id,
            "run",
            &run.id,
            "run.status_changed",
            "system",
            None,
            serde_json::json!({"from": run.status, "to": RunStatus::Succeeded, "taskId": run.task_id}),
            now,
        )?;
        let artifact_id = uuid::Uuid::new_v4().to_string();
        let artifact_type = if message_turn {
            "message"
        } else {
            match task.workspace_mode {
                WorkspaceMode::ReadOnly => "review",
                WorkspaceMode::Worktree => "git_commit",
                WorkspaceMode::SharedWrite => "shared_workspace",
            }
        };
        tx.execute(
            "INSERT INTO ma_artifacts
             (id, project_id, task_id, run_id, artifact_type, uri, summary, content_hash, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![artifact_id, run.project_id, run.task_id, run.id, artifact_type, workspace_uri, summary, content_hash, now],
        )?;
        let message_id = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO ma_messages
             (id, project_id, task_id, sender_type, sender_id, target_agent_id, body, created_at)
             VALUES (?1, ?2, ?3, 'agent', ?4, ?5, ?6, ?7)",
            params![
                message_id,
                run.project_id,
                run.task_id,
                run.agent_id,
                task.reviewer_agent_id,
                summary,
                now
            ],
        )?;
        let is_watchdog_review = super::governance::watchdog_target(&tx, &run.task_id)?.is_some();
        let managed = managed_target.is_some();
        let next_status = if message_turn {
            Some("done")
        } else if producer.role == AgentRole::Reviewer {
            if is_watchdog_review {
                Some("done")
            } else {
                match review_decision
                    .as_ref()
                    .expect("validated reviewer decision")
                    .verdict
                {
                    ReviewVerdict::Approved => Some("done"),
                    ReviewVerdict::ChangesRequested => None,
                }
            }
        } else if acceptance_verified
            && task.reviewer_agent_id.is_none()
            && (producer.role != AgentRole::Integrator || managed)
        {
            Some("done")
        } else {
            Some("in_review")
        };

        if let Some(decision) = review_decision.as_ref().filter(|_| is_watchdog_review) {
            let target = get_task_with(&tx, &decision.target_task_id)?;
            super::governance::apply_watchdog_decision(&tx, decision, &target, now)?;
            tx.execute(
                "UPDATE ma_review_decisions SET applied_at = ?2 WHERE id = ?1",
                params![decision.id, now],
            )?;
        }

        if let Some(decision) = review_decision.as_ref().filter(|_| !is_watchdog_review) {
            match decision.verdict {
                ReviewVerdict::Approved => append_event(
                    &tx,
                    &run.project_id,
                    "review",
                    &decision.id,
                    "review.approved",
                    "agent",
                    Some(&run.agent_id),
                    serde_json::json!({
                        "reviewTaskId": run.task_id,
                        "targetTaskId": decision.target_task_id,
                    }),
                    now,
                )?,
                ReviewVerdict::ChangesRequested => {
                    let target = get_task_with(&tx, &decision.target_task_id)?;
                    let target_attempt: u32 = tx.query_row(
                        "SELECT COALESCE(MAX(attempt), 0) FROM ma_runs WHERE task_id = ?1",
                        params![target.id],
                        |row| row.get(0),
                    )?;
                    let target_charged_attempt = delegation::charged_attempt(&tx, &target.id, target_attempt)?;
                    let can_retry =
                        run.attempt < task.max_attempts && target_charged_attempt < target.max_attempts;
                    let feedback_message_id = uuid::Uuid::new_v4().to_string();
                    tx.execute(
                        "INSERT INTO ma_messages
                         (id, project_id, task_id, sender_type, sender_id, target_agent_id, body, created_at)
                         VALUES (?1, ?2, ?3, 'reviewer', ?4, ?5, ?6, ?7)",
                        params![
                            feedback_message_id,
                            run.project_id,
                            target.id,
                            run.agent_id,
                            target.assignee_agent_id,
                            decision.summary,
                            now,
                        ],
                    )?;
                    if can_retry {
                        tx.execute(
                            "UPDATE ma_tasks SET status = 'changes_requested', next_retry_at = NULL,
                                    updated_at = ?2 WHERE id = ?1",
                            params![target.id, now],
                        )?;
                        tx.execute(
                            "UPDATE ma_tasks SET status = 'blocked', next_retry_at = NULL,
                                    updated_at = ?2 WHERE id = ?1",
                            params![run.task_id, now],
                        )?;
                        // Reviewer 必须创建新只读工作区，旧工作区不含实现者的修复提交。
                        if !pair_review {
                            tx.execute("UPDATE ma_runs SET session_locator_json = NULL WHERE task_id = ?1",
                                params![run.task_id])?;
                        }
                        append_event(
                            &tx,
                            &run.project_id,
                            "review",
                            &decision.id,
                            "review.changes_requested",
                            "agent",
                            Some(&run.agent_id),
                            serde_json::json!({
                                "reviewTaskId": run.task_id,
                                "targetTaskId": target.id,
                                "targetAttempt": target_attempt,
                                "reviewAttempt": run.attempt,
                                "feedbackMessageId": feedback_message_id,
                            }),
                            now,
                        )?;
                    } else {
                        tx.execute(
                            "UPDATE ma_tasks SET status = 'failed', next_retry_at = NULL,
                                    updated_at = ?2 WHERE id IN (?1, ?3)",
                            params![target.id, now, run.task_id],
                        )?;
                        append_event(
                            &tx,
                            &run.project_id,
                            "review",
                            &decision.id,
                            "review.loop_exhausted",
                            "agent",
                            Some(&run.agent_id),
                            serde_json::json!({
                                "reviewTaskId": run.task_id,
                                "targetTaskId": target.id,
                                "targetAttempt": target_attempt,
                                "targetMaxAttempts": target.max_attempts,
                                "reviewAttempt": run.attempt,
                                "reviewMaxAttempts": task.max_attempts,
                                "feedbackMessageId": feedback_message_id,
                            }),
                            now,
                        )?;
                    }
                }
            }
            tx.execute(
                "UPDATE ma_review_decisions SET applied_at = ?2 WHERE id = ?1",
                params![decision.id, now],
            )?;
        }

        if let Some(next_status) = next_status {
            tx.execute(
                "UPDATE ma_tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![run.task_id, next_status, now],
            )?;
            append_event(
                &tx,
                &run.project_id,
                "task",
                &run.task_id,
                "task.execution_completed",
                "agent",
                Some(&run.agent_id),
                serde_json::json!({"status": next_status, "artifactId": artifact_id}),
                now,
            )?;
            if next_status == "done" {
                unblock_ready_tasks(&tx, &run.project_id, now)?;
            } else {
                let approval_id = uuid::Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO ma_approvals
                     (id, project_id, task_id, run_id, approval_type, summary, status, created_at)
                     VALUES (?1, ?2, ?3, ?4, 'task_review', ?5, 'pending', ?6)",
                    params![
                        approval_id,
                        run.project_id,
                        run.task_id,
                        run.id,
                        format!(
                            "确认任务验收：{}（{}）",
                            task.title, task.acceptance_criteria
                        ),
                        now
                    ],
                )?;
            }
        }
        tx.execute(
            "UPDATE ma_memory_entries SET verified = 1, updated_at = ?2
             WHERE source_run_id = ?1",
            params![run.id, now],
        )?;
        tx.execute(
            "INSERT INTO ma_memory_entries
             (id, project_id, namespace, key, value, source_task_id, source_run_id,
              confidence, verified, created_at, updated_at)
             VALUES (?1, ?2, 'outcome', ?3, ?4, ?5, ?6, 1.0, 1, ?7, ?7)
             ON CONFLICT(project_id, namespace, key, source_run_id) DO UPDATE SET
               value = excluded.value, confidence = 1.0, verified = 1,
               updated_at = excluded.updated_at",
            params![
                uuid::Uuid::new_v4().to_string(),
                run.project_id,
                format!("task:{}:attempt:{}", run.task_id, run.attempt),
                summary,
                run.task_id,
                run.id,
                now,
            ],
        )?;
        tx.execute(
            "INSERT INTO ma_runtime_events
             (project_id, run_id, task_id, agent_id, event_type, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, 'completed', ?5, ?6)",
            params![
                run.project_id,
                run.id,
                run.task_id,
                run.agent_id,
                {
                    let mut receipt = serde_json::json!({
                    "summary": summary,
                    "acceptanceVerified": acceptance_verified,
                    "artifactId": artifact_id,
                    });
                    if managed && acceptance_verified {
                        receipt["verificationCommand"] = serde_json::json!(task.verification_command);
                    }
                    receipt.to_string()
                },
                now,
            ],
        )?;
        tx.commit()?;
        Ok(())
    }

    pub fn approve_task(&self, task_id: &str) -> Result<MultiAgentTask> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = get_task_with(&tx, task_id)?;
        let now = now_millis();
        if task.status == TaskStatus::WaitingApproval {
            let run_id: String = tx
                .query_row(
                    "SELECT id FROM ma_runs
                     WHERE task_id = ?1 AND status = 'waiting_approval'
                     ORDER BY attempt DESC LIMIT 1",
                    params![task_id],
                    |row| row.get(0),
                )
                .optional()?
                .ok_or_else(|| anyhow!("任务没有等待审批的活动 Run"))?;
            let pending: i64 = tx.query_row(
                "SELECT COUNT(*) FROM ma_approvals WHERE run_id=?1 AND status='pending'",
                [&run_id], |row| row.get(0),
            )?;
            if pending > 1 {
                return Err(anyhow!("此任务有多项待审批请求，请在干预中心逐项处理"));
            }
            tx.execute(
                "UPDATE ma_runs SET status = 'running', lease_expires_at = ?2,
                        last_heartbeat_at = ?3
                 WHERE id = ?1",
                params![run_id, now.saturating_add(120_000), now],
            )?;
            tx.execute(
                "UPDATE ma_tasks SET status = 'running', updated_at = ?2 WHERE id = ?1",
                params![task_id, now],
            )?;
            tx.execute(
                "UPDATE ma_approvals SET status = 'approved', decided_at = ?2
                 WHERE run_id = ?1 AND status = 'pending'",
                params![run_id, now],
            )?;
            tx.execute(
                "UPDATE ma_agent_sessions SET status = 'running', updated_at = ?2
                 WHERE current_run_id = ?1",
                params![run_id, now],
            )?;
            append_event(
                &tx,
                &task.project_id,
                "task",
                task_id,
                "approval.granted",
                "user",
                None,
                serde_json::json!({"runId": run_id}),
                now,
            )?;
            tx.commit()?;
            return get_task_with(&connection, task_id);
        }
        if task.status != TaskStatus::InReview {
            return Err(anyhow!("只有等待审批或待审查任务可以批准"));
        }
        if let Some(delivery) = planned_worker_delivery(&tx, &task)? {
            let project = self.get_project_with(&tx, &task.project_id)?;
            super::executor::validate_planned_delivery(&project, &delivery.workspace, &delivery.hash, &delivery.scopes)?;
        }
        tx.execute(
            "UPDATE ma_tasks SET status = 'done', updated_at = ?2 WHERE id = ?1",
            params![task_id, now],
        )?;
        tx.execute(
            "UPDATE ma_approvals SET status = 'approved', decided_at = ?2
             WHERE task_id = ?1 AND status = 'pending'",
            params![task_id, now],
        )?;
        unblock_ready_tasks(&tx, &task.project_id, now)?;
        append_event(
            &tx,
            &task.project_id,
            "task",
            task_id,
            "task.approved",
            "user",
            None,
            serde_json::json!({}),
            now,
        )?;
        tx.commit()?;
        get_task_with(&connection, task_id)
    }

    pub fn update_resumed_delivery(&self, task_id: &str) -> Result<MultiAgentTask> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = get_task_with(&tx, task_id)?;
        anyhow::ensure!(matches!(task.status, TaskStatus::InReview | TaskStatus::Done), "仅可补登记待审核或已批准的开发交付");
        let delivery = planned_worker_delivery(&tx, &task)?
            .context("此任务没有可补登记的原工作树交付")?;
        let approved = task.status == TaskStatus::Done;
        let approval_status = if approved { "approved" } else { "pending" };
        let approval_id: String = tx.query_row(
            "SELECT id FROM ma_approvals WHERE task_id=?1 AND run_id=?2 AND approval_type='task_review' AND status=?3
             ORDER BY created_at DESC,rowid DESC LIMIT 1",
            params![task_id, delivery.run_id, approval_status],
            |row| row.get(0),
        ).optional()?.context("原交付审批不可用，请刷新任务记录")?;
        let dependent_started: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_task_dependencies d JOIN ma_tasks t ON t.id=d.task_id
             WHERE d.depends_on_task_id=?1 AND (t.status NOT IN ('draft','ready','blocked')
             OR EXISTS(SELECT 1 FROM ma_runs r WHERE r.task_id=t.id)))",
            [task_id], |row| row.get(0),
        )?;
        anyhow::ensure!(!dependent_started, "后续任务已开始，请走审查退回流程更新交付");
        let project = self.get_project_with(&tx, &task.project_id)?;
        let hash = super::executor::checkpoint_resumed_delivery(
            &project, &task, &delivery.workspace, &delivery.hash, &delivery.scopes,
        )?;
        let now = now_millis();
        tx.execute("UPDATE ma_artifacts SET content_hash=?2 WHERE id=?1", params![delivery.artifact_id, hash])?;
        tx.execute("UPDATE ma_tasks SET status='in_review',updated_at=?2 WHERE id=?1", params![task_id, now])?;
        if approved {
            tx.execute(
                "INSERT INTO ma_approvals (id,project_id,task_id,run_id,approval_type,summary,status,created_at)
                 SELECT ?2,project_id,task_id,run_id,approval_type,summary,'pending',?3 FROM ma_approvals WHERE id=?1",
                params![approval_id, uuid::Uuid::new_v4().to_string(), now],
            )?;
            tx.execute(
                "UPDATE ma_tasks SET status='blocked',updated_at=?2 WHERE status='ready'
                 AND id IN (SELECT task_id FROM ma_task_dependencies WHERE depends_on_task_id=?1)",
                params![task_id, now],
            )?;
        }
        append_event(&tx, &task.project_id, "task", task_id, "task.delivery_updated", "user", None,
            serde_json::json!({"runId": delivery.run_id, "previousCommit": delivery.hash, "commit": hash}), now)?;
        tx.commit()?;
        get_task_with(&connection, task_id)
    }

    pub fn integrator_delivery(&self, task_id: &str) -> Result<Option<(String, String, String)>> {
        let connection = self.connection.lock().unwrap();
        let task = get_task_with(&connection, task_id)?;
        if task.status != TaskStatus::InReview {
            return Ok(None);
        }
        let Some(agent_id) = task.assignee_agent_id.as_deref() else {
            return Ok(None);
        };
        if get_agent_with(&connection, agent_id)?.role != AgentRole::Integrator {
            return Ok(None);
        }
        let project = self.get_project_with(&connection, &task.project_id)?;
        let delivery: Option<(String, Option<String>)> = connection
            .query_row(
                "SELECT uri, content_hash FROM ma_artifacts
                 WHERE task_id = ?1 AND artifact_type IN ('git_commit', 'workspace')
                 ORDER BY created_at DESC LIMIT 1",
                params![task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((workspace, content_hash)) = delivery else {
            return Err(anyhow!("集成任务缺少可审计的 Git 交付，无法批准"));
        };
        let content_hash = content_hash
            .filter(|hash| !hash.trim().is_empty())
            .ok_or_else(|| anyhow!("集成任务交付缺少提交哈希，无法批准"))?;
        Ok(Some((project.root_path, workspace, content_hash)))
    }

    pub fn retry_task(&self, task_id: &str) -> Result<MultiAgentTask> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = get_task_with(&tx, task_id)?;
        if !matches!(
            task.status,
            TaskStatus::Failed
                | TaskStatus::Blocked
                | TaskStatus::Cancelled
                | TaskStatus::ChangesRequested
        ) {
            return Err(anyhow!("任务当前状态不可重试: {}", task.status.as_str()));
        }
        for agent_id in [task.assignee_agent_id.as_deref(), task.reviewer_agent_id.as_deref()].into_iter().flatten() {
            ensure_current_agent_in_project(&tx, agent_id, &task.project_id)?;
        }
        let incomplete: i64 = tx.query_row(
            "SELECT COUNT(*) FROM ma_task_dependencies d
             JOIN ma_tasks dependency ON dependency.id = d.depends_on_task_id
             WHERE d.task_id = ?1 AND dependency.status != 'done'",
            params![task_id],
            |row| row.get(0),
        )?;
        let status = if incomplete == 0 { "ready" } else { "blocked" };
        let now = now_millis();
        tx.execute(
            "UPDATE ma_tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![task_id, status, now],
        )?;
        append_event(
            &tx,
            &task.project_id,
            "task",
            task_id,
            "task.retry_requested",
            "user",
            None,
            serde_json::json!({"status": status}),
            now,
        )?;
        tx.commit()?;
        get_task_with(&connection, task_id)
    }

    pub fn request_task_changes(&self, task_id: &str, reason: &str) -> Result<MultiAgentTask> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = get_task_with(&tx, task_id)?;
        for agent_id in [task.assignee_agent_id.as_deref(), task.reviewer_agent_id.as_deref()].into_iter().flatten() {
            ensure_current_agent_in_project(&tx, agent_id, &task.project_id)?;
        }
        let now = now_millis();
        tx.execute(
            "UPDATE ma_tasks SET status = 'changes_requested',
             description = description || char(10) || char(10) || '集成反馈：' || ?2,
             updated_at = ?3 WHERE id = ?1",
            params![task_id, reason, now],
        )?;
        append_event(
            &tx,
            &task.project_id,
            "task",
            task_id,
            "task.changes_requested",
            "system",
            None,
            serde_json::json!({"reason": reason}),
            now,
        )?;
        tx.commit()?;
        get_task_with(&connection, task_id)
    }

    pub fn reassign_task(&self, task_id: &str, agent_id: &str) -> Result<MultiAgentTask> {
        let profile = self.agent_profile(agent_id)?;
        if !profile.enabled || profile.runtime_kind.is_retired() {
            return Err(anyhow!("目标 Agent 已禁用或其 Runtime 已移除，不能改派"));
        }
        let _runtime_lease = crate::runtime_preferences::acquire(profile.runtime_kind)?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = get_task_with(&tx, task_id)?;
        if let Some(reviewer_id) = &task.reviewer_agent_id {
            ensure_current_agent_in_project(&tx, reviewer_id, &task.project_id)?;
        }
        ensure_agent_in_project(&tx, agent_id, &task.project_id)?;
        let agent = get_agent_with(&tx, agent_id)?;
        if !agent.enabled || agent.runtime_kind.is_retired() {
            return Err(anyhow!("目标 Agent 已禁用或其 Runtime 已移除，不能改派"));
        }
        if agent.runtime_kind == AgentRuntimeKind::Antigravity {
            return Err(anyhow!("Antigravity CLI 目前仅支持团队普通对话"));
        }
        if matches!(
            task.status,
            TaskStatus::Claimed
                | TaskStatus::Running
                | TaskStatus::WaitingApproval
                | TaskStatus::Done
        ) {
            return Err(anyhow!("运行中、待批准或已完成任务不能直接重派"));
        }
        let now = now_millis();
        tx.execute(
            "UPDATE ma_tasks SET assignee_agent_id = ?2,
             status = CASE WHEN status IN ('failed','cancelled','changes_requested') THEN 'ready' ELSE status END,
             updated_at = ?3 WHERE id = ?1",
            params![task_id, agent_id, now],
        )?;
        tx.execute("UPDATE ma_wakeups SET status = 'cancelled', finished_at = ?2 WHERE task_id = ?1 AND status IN ('queued','claimed')",
            params![task_id, now])?;
        append_event(
            &tx,
            &task.project_id,
            "task",
            task_id,
            "task.reassigned",
            "user",
            Some(agent_id),
            serde_json::json!({"agentId": agent_id}),
            now,
        )?;
        tx.commit()?;
        get_task_with(&connection, task_id)
    }

    /// Atomically cancel only exact automation message tasks that have never started.
    pub fn cancel_queued_automation_tasks(&self, targets: &[crate::automation::AutomationTarget]) -> Result<()> {
        let mut connection=self.connection.lock().unwrap();
        let tx=connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for target in targets {
            if !target.message_id.starts_with("automation-") { anyhow::bail!("不是自动化消息任务"); }
            let id=format!("cli-message-{}",target.message_id);
            if target.task_id.as_ref()!=Some(&id) { anyhow::bail!("任务身份不匹配"); }
            let task=get_task_with(&tx,&id)?;
            if Some(&task.project_id)!=target.project_id.as_ref() || !matches!(task.status,TaskStatus::Ready|TaskStatus::Blocked|TaskStatus::Draft|TaskStatus::ChangesRequested) { anyhow::bail!("本轮任务已经启动，需按具体 Run 停止"); }
            let active:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_runs WHERE task_id=?1 AND status IN ('queued','starting','running','waiting_input','waiting_approval'))",[&id],|r|r.get(0))?;
            if active { anyhow::bail!("本轮存在活动 Run，拒绝只改任务状态"); }
        }
        for target in targets {
            let id=target.task_id.as_ref().unwrap();
            tx.execute("UPDATE ma_tasks SET status='cancelled',updated_at=?2 WHERE id=?1",params![id,now_millis()])?;
            tx.execute("UPDATE ma_wakeups SET status='cancelled',finished_at=?2 WHERE task_id=?1 AND status='queued'",params![id,now_millis()])?;
            append_event(&tx,target.project_id.as_ref().unwrap(),"task",id,"automation.cancelled","user",None,serde_json::json!({"messageId":target.message_id}),now_millis())?;
        }
        tx.commit()?;Ok(())
    }

    pub fn cancel_project_tasks(&self, project_id: &str) -> Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_project_exists(&tx, project_id)?;
        let uncertain: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_runs WHERE project_id = ?1 AND COALESCE(json_extract(session_locator_json, '$.stopUnverified'), 0) = 1)", [project_id], |row| row.get(0))?;
        if uncertain { return Err(anyhow!("项目有停止未确认的 Run，请先确认原进程已停止并解除冻结")); }
        let now = now_millis();
        tx.execute(
            "UPDATE ma_runs SET status = 'cancelled', finished_at = ?2, error = '用户取消项目'
             WHERE project_id = ?1 AND status IN ('queued','starting','running','waiting_input','waiting_approval')",
            params![project_id, now],
        )?;
        let count = tx.execute(
            "UPDATE ma_tasks SET status = 'cancelled', updated_at = ?2
             WHERE project_id = ?1 AND status NOT IN ('done','cancelled')",
            params![project_id, now],
        )?;
        tx.execute(
            "UPDATE ma_projects SET status = 'cancelled', updated_at = ?2 WHERE id = ?1",
            params![project_id, now],
        )?;
        tx.execute(
            "UPDATE ma_agent_sessions SET status = 'stopped', updated_at = ?2
             WHERE project_id = ?1 AND status IN ('starting','running','waiting')",
            params![project_id, now],
        )?;
        append_event(
            &tx,
            project_id,
            "project",
            project_id,
            "project.cancelled",
            "user",
            None,
            serde_json::json!({"taskCount": count}),
            now,
        )?;
        tx.commit()?;
        Ok(count)
    }

    pub fn pause_run(&self, run_id: &str) -> Result<AgentRun> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current = get_run_with(&tx, run_id)?;
        if current.session_locator.as_ref().is_some_and(|locator| locator["stopUnverified"] == true) {
            return Err(anyhow!("Run 停止未确认，请先确认原进程已停止并解除冻结"));
        }
        if !matches!(
            current.status,
            RunStatus::Queued
                | RunStatus::Starting
                | RunStatus::Running
                | RunStatus::WaitingInput
                | RunStatus::WaitingApproval
        ) {
            return Ok(current);
        }
        let now = now_millis();
        let message_turn = current.task_id.starts_with("cli-message-");
        tx.execute(
            "UPDATE ma_runs SET status = 'interrupted', finished_at = ?2,
             error = ?3 WHERE id = ?1",
            params![run_id, now, if message_turn {
                "消息处理已中断，可能已产生回复；为避免重复发送，不自动重试"
            } else { "项目已暂停，可继续执行" }],
        )?;
        let dependency_blocked: bool = tx.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM ma_task_dependencies d
               JOIN ma_tasks dependency ON dependency.id = d.depends_on_task_id
               WHERE d.task_id = ?1 AND dependency.status != 'done'
             )",
            params![current.task_id],
            |row| row.get(0),
        )?;
        let task_status = if message_turn {
            "failed"
        } else if dependency_blocked {
            "blocked"
        } else {
            "ready"
        };
        tx.execute(
            "UPDATE ma_tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
            params![current.task_id, task_status, now],
        )?;
        tx.execute(
            "UPDATE ma_agent_sessions SET status = 'stopped', updated_at = ?2
             WHERE project_id = ?1 AND agent_id = ?3 AND current_run_id = ?4",
            params![current.project_id, now, current.agent_id, current.id],
        )?;
        append_event(
            &tx,
            &current.project_id,
            "run",
            run_id,
            "run.paused",
            "user",
            None,
            serde_json::json!({ "taskId": current.task_id }),
            now,
        )?;
        tx.commit()?;
        get_run_with(&connection, run_id)
    }

    pub fn recover_interrupted_runs(&self) -> Result<RecoveryReceipt> {
        self.recover_interrupted_runs_with_startup_cleanup(false)
    }

    pub fn recover_interrupted_runs_at_startup(&self) -> Result<RecoveryReceipt> {
        self.recover_interrupted_runs_with_startup_cleanup(true)
    }

    fn recover_interrupted_runs_with_startup_cleanup(
        &self,
        cleanup_orphaned_wakeups: bool,
    ) -> Result<RecoveryReceipt> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement = tx.prepare(
            "SELECT r.id, r.project_id, r.task_id, t.workspace_mode
             FROM ma_runs r JOIN ma_tasks t ON t.id = r.task_id
             JOIN ma_agents a ON a.id = r.agent_id
             WHERE r.status IN ('starting','running','waiting_input','waiting_approval')
               AND a.runtime_kind IN ('claude_code','codex','kimi_code','pi','opencode','deepseek_harness','zcode','zcode_cli','antigravity')
               AND COALESCE(json_extract(r.session_locator_json, '$.stopUnverified'), 0) != 1",
        )?;
        let stale = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        let mut requeued_read_only_tasks = 0;
        let mut blocked_write_tasks = 0;
        let now = now_millis();
        for (run_id, project_id, task_id, workspace_mode) in &stale {
            tx.execute(
                "UPDATE ma_runs SET status = 'interrupted', finished_at = ?2,
                 error = 'CrewVela 重启时运行未完成' WHERE id = ?1",
                params![run_id, now],
            )?;
            let next_status = if task_id.starts_with("cli-message-") {
                // 回复可能已经发出，重放消息会产生重复发送；保留失败记录供人工判断。
                tx.execute("UPDATE ma_runs SET error = '消息处理在重启时中断，投递结果不确定，未自动重试' WHERE id = ?1", params![run_id])?;
                "failed"
            } else if workspace_mode == "read_only" {
                requeued_read_only_tasks += 1;
                "ready"
            } else {
                blocked_write_tasks += 1;
                "blocked"
            };
            tx.execute(
                "UPDATE ma_tasks SET status = ?2, updated_at = ?3 WHERE id = ?1",
                params![task_id, next_status, now],
            )?;
            tx.execute(
                "UPDATE ma_wakeups SET status = 'cancelled', finished_at = ?3
                 WHERE project_id = ?1 AND task_id = ?2 AND status = 'claimed'",
                params![project_id, task_id, now],
            )?;
            tx.execute(
                "UPDATE ma_agent_sessions SET status = 'stopped', updated_at = ?2
                 WHERE current_run_id = ?1",
                params![run_id, now],
            )?;
            append_event(
                &tx,
                project_id,
                "run",
                run_id,
                "run.interrupted_on_recovery",
                "system",
                None,
                serde_json::json!({ "taskId": task_id, "taskStatus": next_status }),
                now,
            )?;
        }
        if cleanup_orphaned_wakeups {
            tx.execute(
                "UPDATE ma_wakeups SET status = 'cancelled', finished_at = ?1
                 WHERE status = 'claimed'
                   AND NOT EXISTS (
                       SELECT 1 FROM ma_runs r
                       WHERE r.task_id = ma_wakeups.task_id
                         AND r.status IN ('starting','running','waiting_input','waiting_approval')
                   )",
                params![now],
            )?;
        }
        tx.commit()?;
        Ok(RecoveryReceipt {
            interrupted_runs: stale.len(),
            requeued_read_only_tasks,
            blocked_write_tasks,
        })
    }

    pub(crate) fn begin_task_handoff(&self, task_id: &str, handoff_id: &str) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = get_task_with(&tx, task_id)?;
        let completed: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_events WHERE event_type = 'task.handed_off' AND json_extract(payload_json, '$.handoffId') = ?1)", [handoff_id], |row| row.get(0))?;
        if completed {
            return Ok(());
        }
        let existing: Option<String> = tx
            .query_row(
                "SELECT handoff_id FROM ma_handoff_locks WHERE project_id = ?1",
                [&task.project_id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(existing) = existing {
            if existing != handoff_id {
                return Err(anyhow!("该项目正在进行另一项交接"));
            }
        } else {
            tx.execute(
                "INSERT INTO ma_handoff_locks(project_id, handoff_id) VALUES (?1, ?2)",
                params![task.project_id, handoff_id],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn release_handoff_lock(&self, project_id: &str, handoff_id: &str) -> Result<()> {
        let connection = self.connection.lock().unwrap();
        connection.execute(
            "DELETE FROM ma_handoff_locks WHERE project_id = ?1 AND handoff_id = ?2",
            params![project_id, handoff_id],
        )?;
        Ok(())
    }

    pub(crate) fn handoff_in_progress(&self, project_id: &str) -> Result<bool> {
        let connection = self.connection.lock().unwrap();
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_handoff_locks WHERE project_id = ?1)",
            [project_id],
            |row| row.get(0),
        )?)
    }

    pub(crate) fn task_handoff_completed(&self, handoff_id: &str) -> Result<bool> {
        let connection = self.connection.lock().unwrap();
        Ok(connection.query_row("SELECT EXISTS(SELECT 1 FROM ma_events WHERE event_type = 'task.handed_off' AND json_extract(payload_json, '$.handoffId') = ?1)", [handoff_id], |row| row.get(0))?)
    }

    /// ACP 标记与交接锁在同一 SQLite 写事务内互斥，旧 PTY 不能冒充当前执行宿主。
    pub(crate) fn mark_acp_execution(&self, run_id: &str) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, run_id)?;
        ensure_no_handoff(&tx, &run.project_id)?;
        tx.execute("UPDATE ma_runs SET session_locator_json = json_set(COALESCE(session_locator_json, '{}'), '$.transport', 'acp') WHERE id = ?1", [run_id])?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn bind_pty_execution(&self, run_id: &str, pty_id: u32) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, run_id)?;
        ensure_no_handoff(&tx, &run.project_id)?;
        tx.execute("UPDATE ma_runs SET session_locator_json = json_set(COALESCE(session_locator_json, '{}'), '$.transport', 'pty', '$.ptyId', ?2) WHERE id = ?1", params![run_id, pty_id])?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn clear_acp_execution(&self, run_id: &str) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, run_id)?;
        ensure_no_handoff(&tx, &run.project_id)?;
        tx.execute("UPDATE ma_runs SET session_locator_json = json_remove(COALESCE(session_locator_json, '{}'), '$.transport', '$.ptyId') WHERE id = ?1", [run_id])?;
        tx.commit()?;
        Ok(())
    }

    pub(super) fn bind_codex_thread(
        &self, run_id: &str, id: &str, storage: crate::codex_storage::CodexStorage, cwd: &Path,
    ) -> Result<Option<String>> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, run_id)?;
        let locator = run.session_locator.as_ref().context("Codex 运行缺少定位")?;
        anyhow::ensure!(uuid::Uuid::parse_str(id).is_ok() && locator["adapter"] == "codex"
            && locator["cwd"].as_str().map(Path::new) == Some(cwd)
            && crate::codex_storage::CodexStorage::from_locator(locator)? == storage
            && locator["threadId"].as_str().is_none_or(|old| old == id), "Codex 原生身份与本次运行不符");
        let changed = tx.execute("UPDATE ma_runs SET session_locator_json=json_set(session_locator_json,'$.threadId',?2)
            WHERE id=?1 AND status IN ('running','waiting_input','waiting_approval')", params![run_id,id])?;
        anyhow::ensure!(changed == 1, "Codex 运行已停止，拒绝登记新会话");
        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_events WHERE project_id=?1 AND entity_type='run'
            AND entity_id=?2 AND actor_type='system' AND event_type='run.codex_thread')",
            params![run.project_id,run.id], |row| row.get(0))?;
        if !exists {
            let mut proof = serde_json::json!({"threadId":id, "cwd":cwd});
            storage.mark(&mut proof);
            append_event(&tx, &run.project_id, "run", run_id, "run.codex_thread", "system", None, proof, now_millis())?;
        }
        let member = tx.query_row("SELECT app_session_id FROM ma_agent_sessions WHERE project_id=?1 AND agent_id=?2",
            params![run.project_id,run.agent_id], |row| row.get(0)).optional()?.flatten();
        tx.commit()?;
        Ok(member)
    }

    pub(super) fn checkpoint_native_dispatch(
        &self, run_id: &str, pty_id: u32, identity: Option<(&str, &str)>,
    ) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, run_id)?;
        ensure_no_handoff(&tx, &run.project_id)?;
        let mut patch = serde_json::json!({"managedPromptDispatched":true});
        if let Some((key, id)) = identity { patch[key] = id.into(); }
        let changed = tx.execute(
            "UPDATE ma_runs SET session_locator_json=json_patch(session_locator_json,?3)
             WHERE id=?1 AND status='running' AND json_extract(session_locator_json,'$.ptyId')=?2
             AND json_extract(session_locator_json,'$.transport')='pty'",
            params![run_id, pty_id, patch.to_string()],
        )?;
        anyhow::ensure!(changed == 1, "当前托管任务的终端归属已变更");
        tx.commit()?;
        Ok(())
    }

    pub(super) fn checkpoint_managed_turn(
        &self, run_id: &str, phase: &str, patch: serde_json::Value, kind: AgentRuntimeKind, cwd: &Path,
    ) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = get_run_with(&tx, run_id)?;
        ensure_no_handoff(&tx, &run.project_id)?;
        let previous = run.session_locator.as_ref().and_then(|locator| locator["managedTurn"].as_str());
        anyhow::ensure!(matches!((previous, phase), (None, "dispatched") | (Some("dispatched"), "accepted" | "rejected")
            | (Some("accepted"), "completed" | "failed" | "interrupted")), "托管任务的原生轮次状态不连续：{previous:?} → {phase}");
        let mut patch = patch.as_object().context("缺少托管轮次记录")?.clone();
        let prior = run.session_locator.as_ref().context("当前执行没有持久化的授权位置")?;
        for key in ["sessionId","threadId","sessionPath","engine","toolPolicy","commandId","turnId","messageId"] {
            if let (Some(saved),Some(reported)) = (prior.get(key),patch.get(key)) {
                anyhow::ensure!(saved == reported, "原生回执的 {key} 与本轮登记的身份不符");
            }
        }
        if matches!(phase, "accepted" | "completed") {
            anyhow::ensure!(["sessionId","threadId"].iter().any(|key| patch.get(*key).or_else(|| prior.get(*key))
                .and_then(serde_json::Value::as_str).is_some_and(|id| !id.is_empty())), "受理回执缺少原生会话身份");
        }
        anyhow::ensure!(!["cwd", "adapter", "managedBaseline", "managedLaunchMode", "managedTurn", "stopUnverified"]
            .iter().any(|key| patch.contains_key(*key)), "原生回执不能更改任务授权或工作树");
        // Keep the identifiers reported by the adapter separately from the editable run locator.
        let identity: serde_json::Map<String, serde_json::Value> = ["sessionId", "threadId", "turnId", "commandId", "messageId", "engine", "toolPolicy"]
            .iter().filter_map(|&key| patch.get(key).map(|value| (key.to_owned(), value.clone()))).collect();
        patch.insert("managedTurn".into(), phase.into());
        patch.insert("managedPromptDispatched".into(), true.into());
        let changed = tx.execute(
            "UPDATE ma_runs SET session_locator_json=json_patch(session_locator_json,?2) WHERE id=?1 AND status='running'",
            params![run_id, serde_json::Value::Object(patch).to_string()],
        )?;
        anyhow::ensure!(changed == 1, "当前托管任务已停止，拒绝登记原生回执");
        tx.execute("INSERT INTO ma_runtime_events(project_id,run_id,task_id,agent_id,event_type,payload_json,created_at)
            VALUES(?1,?2,?3,?4,'managed_turn',?5,?6)", params![run.project_id,run.id,run.task_id,run.agent_id,
                serde_json::json!({"phase":phase}).to_string(),now_millis()])?;
        append_event(&tx, &run.project_id, "run", &run.id, "run.managed_history", "system", None,
            serde_json::json!({"phase":phase,"taskId":run.task_id,"agentId":run.agent_id,"runtime":kind,"cwd":cwd,"identity":identity}),now_millis())?;
        tx.commit()?;
        Ok(())
    }

    pub(crate) fn finish_task_handoff(
        &self,
        task_id: &str,
        agent_id: &str,
        original_agent: Option<&str>,
        handoff_id: &str,
        body: &str,
    ) -> Result<MultiAgentTask> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let task = get_task_with(&tx, task_id)?;
        let already: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_events WHERE event_type = 'task.handed_off' AND json_extract(payload_json, '$.handoffId') = ?1)", [handoff_id], |row| row.get(0))?;
        if already {
            tx.execute(
                "DELETE FROM ma_handoff_locks WHERE project_id = ?1 AND handoff_id = ?2",
                params![task.project_id, handoff_id],
            )?;
            tx.commit()?;
            return Ok(task);
        }
        let lock: Option<String> = tx
            .query_row(
                "SELECT handoff_id FROM ma_handoff_locks WHERE project_id = ?1",
                [&task.project_id],
                |row| row.get(0),
            )
            .optional()?;
        if lock.as_deref() != Some(handoff_id) {
            return Err(anyhow!("交接锁已失效"));
        }
        let project = self.get_project_with(&tx, &task.project_id)?;
        if project.status != ProjectStatus::Paused {
            return Err(anyhow!("项目必须保持暂停，不能交接"));
        }
        let active: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_runs WHERE project_id = ?1 AND status IN ('queued','starting','running','waiting_input','waiting_approval'))", [&task.project_id], |row| row.get(0))?;
        if active {
            return Err(anyhow!("项目仍有活动 Run，未改变负责人"));
        }
        if task.assignee_agent_id.as_deref() != original_agent {
            return Err(anyhow!("任务负责人已变化，请重新预览"));
        }
        if matches!(
            task.status,
            TaskStatus::Done | TaskStatus::WaitingApproval | TaskStatus::InReview
        ) {
            return Err(anyhow!("完成或审核中的任务不能交接"));
        }
        ensure_agent_in_project(&tx, agent_id, &task.project_id)?;
        let agent = get_agent_with(&tx, agent_id)?;
        if !agent.enabled || agent.runtime_kind.is_retired() {
            return Err(anyhow!("接收成员不可用"));
        }
        let now = now_millis();
        tx.execute("UPDATE ma_tasks SET assignee_agent_id = ?2, status = CASE WHEN status IN ('failed','cancelled','changes_requested') THEN 'ready' ELSE status END, updated_at = ?3 WHERE id = ?1", params![task_id, agent_id, now])?;
        tx.execute("UPDATE ma_wakeups SET status = 'cancelled', finished_at = ?2 WHERE task_id = ?1 AND status IN ('queued','claimed')", params![task_id, now])?;
        tx.execute("INSERT INTO ma_agent_instructions(id, project_id, agent_id, task_id, body, status, created_at) VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6)", params![handoff_id, task.project_id, agent_id, task_id, body, now])?;
        append_event(
            &tx,
            &task.project_id,
            "task",
            task_id,
            "task.handed_off",
            "user",
            Some(agent_id),
            serde_json::json!({"handoffId": handoff_id, "fromAgentId": original_agent, "toAgentId": agent_id, "context": body}),
            now,
        )?;
        tx.execute(
            "DELETE FROM ma_handoff_locks WHERE project_id = ?1 AND handoff_id = ?2",
            params![task.project_id, handoff_id],
        )?;
        tx.commit()?;
        get_task_with(&connection, task_id)
    }

    fn get_project_with(
        &self,
        connection: &Connection,
        project_id: &str,
    ) -> Result<MultiAgentProject> {
        connection
            .query_row(
                "SELECT id, name, goal, root_path, default_branch, status, created_at, updated_at
                 FROM ma_projects WHERE id = ?1",
                params![project_id],
                project_from_row,
            )
            .optional()?
            .ok_or_else(|| anyhow!("未找到 Multi-Agent 项目: {project_id}"))
    }
}

fn ensure_no_handoff(connection: &Connection, project_id: &str) -> Result<()> {
    let locked: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM ma_handoff_locks WHERE project_id = ?1)",
        [project_id],
        |row| row.get(0),
    )?;
    if locked {
        return Err(anyhow!("项目正在交接，暂不能启动或领取任务"));
    }
    Ok(())
}

fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch("CREATE TABLE IF NOT EXISTS ma_handoff_locks(project_id TEXT PRIMARY KEY, handoff_id TEXT NOT NULL);")?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS ma_meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ma_projects (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            goal TEXT NOT NULL,
            root_path TEXT NOT NULL,
            default_branch TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ma_agent_avatar_images (
            id TEXT PRIMARY KEY,
            png BLOB NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ma_agents (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            runtime_kind TEXT NOT NULL,
            role TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            model TEXT,
            permission_profile TEXT NOT NULL DEFAULT 'default',
            max_concurrency INTEGER NOT NULL DEFAULT 1,
            capabilities_json TEXT NOT NULL DEFAULT '[]',
            enabled INTEGER NOT NULL DEFAULT 1,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            UNIQUE(project_id, name)
        );
        CREATE TABLE IF NOT EXISTS ma_tasks (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            parent_id TEXT REFERENCES ma_tasks(id) ON DELETE SET NULL,
            stage INTEGER NOT NULL DEFAULT 0,
            title TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            acceptance_criteria TEXT NOT NULL DEFAULT '',
            verification_command TEXT,
            status TEXT NOT NULL,
            assignee_agent_id TEXT REFERENCES ma_agents(id) ON DELETE SET NULL,
            reviewer_agent_id TEXT REFERENCES ma_agents(id) ON DELETE SET NULL,
            workspace_mode TEXT NOT NULL DEFAULT 'worktree',
            priority INTEGER NOT NULL DEFAULT 0,
            required_capabilities_json TEXT NOT NULL DEFAULT '[]',
            max_attempts INTEGER NOT NULL DEFAULT 3,
            retry_backoff_seconds INTEGER NOT NULL DEFAULT 5,
            next_retry_at INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ma_task_dependencies (
            task_id TEXT NOT NULL REFERENCES ma_tasks(id) ON DELETE CASCADE,
            depends_on_task_id TEXT NOT NULL REFERENCES ma_tasks(id) ON DELETE CASCADE,
            PRIMARY KEY(task_id, depends_on_task_id),
            CHECK(task_id != depends_on_task_id)
        );
        CREATE TABLE IF NOT EXISTS ma_runs (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            task_id TEXT NOT NULL REFERENCES ma_tasks(id) ON DELETE CASCADE,
            agent_id TEXT NOT NULL REFERENCES ma_agents(id) ON DELETE RESTRICT,
            attempt INTEGER NOT NULL,
            status TEXT NOT NULL,
            session_locator_json TEXT,
            lease_expires_at INTEGER NOT NULL,
            last_heartbeat_at INTEGER,
            output_summary TEXT,
            error TEXT,
            created_at INTEGER NOT NULL,
            started_at INTEGER,
            finished_at INTEGER,
            UNIQUE(task_id, attempt)
        );
        CREATE TABLE IF NOT EXISTS ma_review_decisions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            review_task_id TEXT NOT NULL REFERENCES ma_tasks(id) ON DELETE CASCADE,
            review_run_id TEXT NOT NULL REFERENCES ma_runs(id) ON DELETE CASCADE,
            reviewer_agent_id TEXT NOT NULL REFERENCES ma_agents(id) ON DELETE RESTRICT,
            target_task_id TEXT NOT NULL REFERENCES ma_tasks(id) ON DELETE CASCADE,
            verdict TEXT NOT NULL CHECK(verdict IN ('approved','changes_requested')),
            summary TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            applied_at INTEGER,
            UNIQUE(review_run_id)
        );
        CREATE TABLE IF NOT EXISTS ma_wakeups (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            agent_id TEXT NOT NULL REFERENCES ma_agents(id) ON DELETE CASCADE,
            task_id TEXT REFERENCES ma_tasks(id) ON DELETE CASCADE,
            run_id TEXT REFERENCES ma_runs(id) ON DELETE CASCADE,
            reason TEXT NOT NULL,
            status TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            coalesced_count INTEGER NOT NULL DEFAULT 0,
            requested_at INTEGER NOT NULL,
            claimed_at INTEGER,
            finished_at INTEGER
        );
        CREATE UNIQUE INDEX IF NOT EXISTS ma_wakeups_active_idempotency
            ON ma_wakeups(idempotency_key)
            WHERE status IN ('queued','claimed');
        CREATE TABLE IF NOT EXISTS ma_events (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            entity_type TEXT NOT NULL,
            entity_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            actor_type TEXT NOT NULL,
            actor_id TEXT,
            payload_json TEXT NOT NULL DEFAULT '{}',
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ma_messages (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            task_id TEXT REFERENCES ma_tasks(id) ON DELETE CASCADE,
            sender_type TEXT NOT NULL,
            sender_id TEXT,
            target_agent_id TEXT REFERENCES ma_agents(id) ON DELETE SET NULL,
            body TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ma_artifacts (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            task_id TEXT NOT NULL REFERENCES ma_tasks(id) ON DELETE CASCADE,
            run_id TEXT NOT NULL REFERENCES ma_runs(id) ON DELETE CASCADE,
            artifact_type TEXT NOT NULL,
            uri TEXT NOT NULL,
            summary TEXT NOT NULL DEFAULT '',
            content_hash TEXT,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ma_approvals (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            task_id TEXT REFERENCES ma_tasks(id) ON DELETE CASCADE,
            run_id TEXT REFERENCES ma_runs(id) ON DELETE CASCADE,
            approval_type TEXT NOT NULL,
            summary TEXT NOT NULL,
            status TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            decided_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS ma_agent_sessions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            agent_id TEXT NOT NULL REFERENCES ma_agents(id) ON DELETE CASCADE,
            current_task_id TEXT REFERENCES ma_tasks(id) ON DELETE SET NULL,
            current_run_id TEXT REFERENCES ma_runs(id) ON DELETE SET NULL,
            app_session_id TEXT,
            status TEXT NOT NULL DEFAULT 'idle',
            control_mode TEXT NOT NULL DEFAULT 'auto',
            last_output_at INTEGER,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            UNIQUE(project_id, agent_id)
        );
        CREATE TABLE IF NOT EXISTS ma_run_logs (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            run_id TEXT NOT NULL REFERENCES ma_runs(id) ON DELETE CASCADE,
            task_id TEXT NOT NULL REFERENCES ma_tasks(id) ON DELETE CASCADE,
            agent_id TEXT NOT NULL REFERENCES ma_agents(id) ON DELETE CASCADE,
            stream TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ma_agent_instructions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            agent_id TEXT NOT NULL REFERENCES ma_agents(id) ON DELETE CASCADE,
            task_id TEXT REFERENCES ma_tasks(id) ON DELETE SET NULL,
            body TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'queued',
            created_at INTEGER NOT NULL,
            delivered_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS ma_runtime_events (
            seq INTEGER PRIMARY KEY AUTOINCREMENT,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            run_id TEXT NOT NULL REFERENCES ma_runs(id) ON DELETE CASCADE,
            task_id TEXT NOT NULL REFERENCES ma_tasks(id) ON DELETE CASCADE,
            agent_id TEXT NOT NULL REFERENCES ma_agents(id) ON DELETE CASCADE,
            event_type TEXT NOT NULL,
            payload_json TEXT NOT NULL DEFAULT '{}',
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS ma_memory_entries (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            namespace TEXT NOT NULL,
            key TEXT NOT NULL,
            value TEXT NOT NULL,
            source_task_id TEXT REFERENCES ma_tasks(id) ON DELETE SET NULL,
            source_run_id TEXT REFERENCES ma_runs(id) ON DELETE SET NULL,
            confidence REAL NOT NULL DEFAULT 0.5,
            verified INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            UNIQUE(project_id, namespace, key, source_run_id)
        );
        CREATE TABLE IF NOT EXISTS ma_knowledge_documents (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            source_type TEXT NOT NULL,
            source_id TEXT NOT NULL,
            task_id TEXT REFERENCES ma_tasks(id) ON DELETE CASCADE,
            run_id TEXT REFERENCES ma_runs(id) ON DELETE CASCADE,
            title TEXT NOT NULL,
            content TEXT NOT NULL,
            source_uri TEXT NOT NULL,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            UNIQUE(project_id, source_type, source_id)
        );
        CREATE TABLE IF NOT EXISTS ma_interventions (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            task_id TEXT REFERENCES ma_tasks(id) ON DELETE CASCADE,
            run_id TEXT REFERENCES ma_runs(id) ON DELETE CASCADE,
            kind TEXT NOT NULL,
            severity TEXT NOT NULL,
            title TEXT NOT NULL,
            summary TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'open',
            recommended_action TEXT NOT NULL,
            fingerprint TEXT NOT NULL UNIQUE,
            snoozed_until INTEGER,
            metadata_json TEXT NOT NULL DEFAULT '{}',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            resolved_at INTEGER
        );
        CREATE TABLE IF NOT EXISTS ma_semantic_watchdogs (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            review_task_id TEXT NOT NULL UNIQUE REFERENCES ma_tasks(id) ON DELETE CASCADE,
            target_task_id TEXT NOT NULL REFERENCES ma_tasks(id) ON DELETE CASCADE,
            reviewer_agent_id TEXT NOT NULL REFERENCES ma_agents(id) ON DELETE RESTRICT,
            fingerprint TEXT NOT NULL UNIQUE,
            status TEXT NOT NULL DEFAULT 'scheduled',
            summary TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            reviewed_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS ma_tasks_project_status ON ma_tasks(project_id, status);
        CREATE INDEX IF NOT EXISTS ma_runs_project_status ON ma_runs(project_id, status);
        CREATE INDEX IF NOT EXISTS ma_review_decisions_project_created
            ON ma_review_decisions(project_id, created_at);
        CREATE INDEX IF NOT EXISTS ma_events_project_seq ON ma_events(project_id, seq);
        CREATE INDEX IF NOT EXISTS ma_wakeups_project_status ON ma_wakeups(project_id, status);
        CREATE INDEX IF NOT EXISTS ma_messages_project_created ON ma_messages(project_id, created_at);
        CREATE INDEX IF NOT EXISTS ma_artifacts_project_created ON ma_artifacts(project_id, created_at);
        CREATE INDEX IF NOT EXISTS ma_approvals_project_status ON ma_approvals(project_id, status);
        CREATE TRIGGER IF NOT EXISTS ma_close_run_approvals
        AFTER UPDATE OF status ON ma_runs
        WHEN NEW.status IN ('succeeded','failed','timed_out','cancelled','interrupted')
        BEGIN
            UPDATE ma_approvals SET status='cancelled', decided_at=NEW.finished_at
            WHERE run_id=NEW.id AND status='pending'
              AND NOT (NEW.status='succeeded' AND approval_type='task_review');
        END;
        CREATE INDEX IF NOT EXISTS ma_agent_sessions_project ON ma_agent_sessions(project_id, updated_at);
        CREATE INDEX IF NOT EXISTS ma_run_logs_project_seq ON ma_run_logs(project_id, seq);
        CREATE INDEX IF NOT EXISTS ma_run_logs_run_seq ON ma_run_logs(run_id, seq);
        CREATE INDEX IF NOT EXISTS ma_agent_instructions_agent_status
            ON ma_agent_instructions(agent_id, status, created_at);
        CREATE INDEX IF NOT EXISTS ma_runtime_events_project_seq
            ON ma_runtime_events(project_id, seq);
        CREATE INDEX IF NOT EXISTS ma_runtime_events_run_seq
            ON ma_runtime_events(run_id, seq);
        CREATE INDEX IF NOT EXISTS ma_memory_project_namespace
            ON ma_memory_entries(project_id, namespace, verified, updated_at);
        CREATE INDEX IF NOT EXISTS ma_knowledge_project_updated
            ON ma_knowledge_documents(project_id, updated_at DESC);
        CREATE INDEX IF NOT EXISTS ma_knowledge_project_source
            ON ma_knowledge_documents(project_id, source_type, source_id);
        CREATE INDEX IF NOT EXISTS ma_knowledge_project_task
            ON ma_knowledge_documents(project_id, task_id, updated_at DESC);
        CREATE INDEX IF NOT EXISTS ma_interventions_status_priority
            ON ma_interventions(status, severity, updated_at);
        CREATE INDEX IF NOT EXISTS ma_interventions_project_status
            ON ma_interventions(project_id, status, updated_at);
        CREATE INDEX IF NOT EXISTS ma_watchdogs_project_status
            ON ma_semantic_watchdogs(project_id, status, updated_at);
        ",
    )?;
    migrate_knowledge_fts(connection)?;
    ensure_column(connection, "ma_agents", "avatar_id", "TEXT")?;
    ensure_column(connection, "ma_agents", "removed_at", "INTEGER")?;
    ensure_column(
        connection,
        "ma_agents",
        "persona",
        "TEXT NOT NULL DEFAULT 'efficient'",
    )?;
    ensure_column(
        connection,
        "ma_agents",
        "persona_custom",
        "TEXT NOT NULL DEFAULT ''",
    )?;
    ensure_column(connection, "ma_tasks", "verification_command", "TEXT")?;
    ensure_column(connection, "ma_artifacts", "content_hash", "TEXT")?;
    ensure_column(connection, "ma_agent_sessions", "app_session_id", "TEXT")?;
    ensure_column(
        connection,
        "ma_agents",
        "capabilities_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        connection,
        "ma_tasks",
        "required_capabilities_json",
        "TEXT NOT NULL DEFAULT '[]'",
    )?;
    ensure_column(
        connection,
        "ma_tasks",
        "max_attempts",
        "INTEGER NOT NULL DEFAULT 3",
    )?;
    ensure_column(
        connection,
        "ma_tasks",
        "retry_backoff_seconds",
        "INTEGER NOT NULL DEFAULT 5",
    )?;
    ensure_column(connection, "ma_tasks", "next_retry_at", "INTEGER")?;
    ensure_column(connection, "ma_runs", "last_heartbeat_at", "INTEGER")?;
    for runtime in [
        AgentRuntimeKind::ClaudeCode,
        AgentRuntimeKind::Codex,
        AgentRuntimeKind::KimiCode,
        AgentRuntimeKind::Pi,
        AgentRuntimeKind::OpenCode,
    ] {
        connection.execute(
            "UPDATE ma_agents SET capabilities_json = ?2
             WHERE runtime_kind = ?1
               AND (capabilities_json IS NULL OR capabilities_json = '' OR capabilities_json = '[]')",
            params![
                runtime.as_str(),
                serde_json::to_string(&super::runtime::capabilities_for_runtime(runtime))?,
            ],
        )?;
    }
    connection.execute(
        "INSERT INTO ma_meta(key, value) VALUES('schema_version', ?1)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![SCHEMA_VERSION.to_string()],
    )?;
    // 版本 3 的会话表只能可靠表示每个 Agent 的一个当前 Run，因此统一收敛为 1。
    connection.execute(
        "UPDATE ma_agents SET max_concurrency = 1 WHERE max_concurrency != 1",
        [],
    )?;
    migrate_brand_references(connection)?;
    Ok(())
}

fn migrate_brand_references(connection: &Connection) -> Result<()> {
    let transaction = connection.unchecked_transaction()?;
    let completed: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM ma_meta WHERE key='crewvela_namespace_v1')", [], |row| row.get(0),
    )?;
    if completed { return Ok(()); }

    let references = {
        let mut statement = transaction.prepare(
            "SELECT id, project_id, source_type, source_id, source_uri FROM ma_knowledge_documents",
        )?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?,
            row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, String>(4)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (id, project_id, source_type, source_id, uri) in references {
        let Some((scheme, tail)) = uri.split_once("://") else { continue; };
        if format!("{:x}", Sha256::digest(scheme.as_bytes()))
            != "e3946634d41c620eba19d52341225a0f0a2c3c58ee487584f654bc4ca4a1c341" { continue; }
        let expected = match source_type.as_str() {
            "project" => format!("projects/{source_id}"),
            "task" => format!("projects/{project_id}/tasks/{source_id}"),
            "conversation" => format!("projects/{project_id}/messages/{source_id}"),
            "run" => format!("projects/{project_id}/runs/{source_id}"),
            "review" => format!("projects/{project_id}/reviews/{source_id}"),
            "memory" => format!("projects/{project_id}/memories/{source_id}"),
            _ => continue,
        };
        if tail == expected {
            transaction.execute("UPDATE ma_knowledge_documents SET source_uri=?1 WHERE id=?2",
                params![format!("crewvela://{tail}"), id])?;
        }
    }

    // Only an exact application-generated message-task contract is owned by
    // this migration. User descriptions, messages and custom criteria remain
    // untouched and cannot serve as evidence of a current protocol handshake.
    let contracts = {
        let mut statement = transaction.prepare(
            "SELECT id, acceptance_criteria FROM ma_tasks WHERE id LIKE 'cli-message-%'",
        )?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (id, contract) in contracts {
        if format!("{:x}", Sha256::digest(contract.as_bytes()))
            == "8237f6ca58fd7f9d5fe960fa3c5c937a295e515a221b1ff892302b1f5d155bb8" {
            transaction.execute("UPDATE ma_tasks SET acceptance_criteria=?1 WHERE id=?2",
                params![MESSAGE_TASK_ACCEPTANCE, id])?;
        }
    }
    transaction.execute("INSERT INTO ma_meta(key,value) VALUES('crewvela_namespace_v1','1')", [])?;
    transaction.commit()?;
    Ok(())
}

fn migrate_knowledge_fts(connection: &Connection) -> Result<()> {
    connection
        .execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS ma_knowledge_fts USING fts5(
                title,
                content,
                content='ma_knowledge_documents',
                content_rowid='rowid',
                tokenize='trigram'
            );
            CREATE TRIGGER IF NOT EXISTS ma_knowledge_documents_ai
            AFTER INSERT ON ma_knowledge_documents BEGIN
                INSERT INTO ma_knowledge_fts(rowid, title, content)
                VALUES (new.rowid, new.title, new.content);
            END;
            CREATE TRIGGER IF NOT EXISTS ma_knowledge_documents_ad
            AFTER DELETE ON ma_knowledge_documents BEGIN
                INSERT INTO ma_knowledge_fts(ma_knowledge_fts, rowid, title, content)
                VALUES ('delete', old.rowid, old.title, old.content);
            END;
            CREATE TRIGGER IF NOT EXISTS ma_knowledge_documents_au
            AFTER UPDATE ON ma_knowledge_documents BEGIN
                INSERT INTO ma_knowledge_fts(ma_knowledge_fts, rowid, title, content)
                VALUES ('delete', old.rowid, old.title, old.content);
                INSERT INTO ma_knowledge_fts(rowid, title, content)
                VALUES (new.rowid, new.title, new.content);
            END;",
        )
        .context("当前 SQLite 不支持 FTS5 trigram，无法初始化项目知识库")?;
    // External-content FTS COUNT(*) reads the source table, even when the index
    // is empty. Repair older installations once, before any UPDATE triggers run.
    let initialized = connection.query_row(
        "SELECT value FROM ma_meta WHERE key = 'knowledge_fts_backfilled_v1'",
        [], |row| row.get::<_, String>(0),
    ).optional()?.is_some();
    if !initialized {
        connection.execute(
            "INSERT INTO ma_knowledge_fts(ma_knowledge_fts) VALUES('rebuild')",
            [],
        )?;
        connection.execute(
            "INSERT INTO ma_meta(key, value) VALUES('knowledge_fts_backfilled_v1', '1')",
            [],
        )?;
    }
    Ok(())
}

fn ensure_column(
    connection: &Connection,
    table: &str,
    column: &str,
    definition: &str,
) -> Result<()> {
    let mut statement = connection.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    if !columns.iter().any(|name| name == column) {
        connection.execute_batch(&format!(
            "ALTER TABLE {table} ADD COLUMN {column} {definition}"
        ))?;
    }
    Ok(())
}

fn sync_project_knowledge_with(connection: &Connection, project_id: &str) -> Result<()> {
    const UPSERT: &str = " ON CONFLICT(project_id, source_type, source_id) DO UPDATE SET
            task_id = excluded.task_id,
            run_id = excluded.run_id,
            title = excluded.title,
            content = excluded.content,
            source_uri = excluded.source_uri,
            metadata_json = excluded.metadata_json,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at";

    connection.execute(
        &format!(
            "INSERT INTO ma_knowledge_documents
             (id, project_id, source_type, source_id, task_id, run_id, title, content,
              source_uri, metadata_json, created_at, updated_at)
             SELECT 'project:' || id, id, 'project', id, NULL, NULL,
                    '项目 · ' || name,
                    name || char(10) || goal || char(10) || root_path || char(10) || default_branch,
                    'crewvela://projects/' || id,
                    '{{}}', created_at, updated_at
             FROM ma_projects WHERE id = ?1{UPSERT}"
        ),
        params![project_id],
    )?;
    connection.execute(
        &format!(
            "INSERT INTO ma_knowledge_documents
             (id, project_id, source_type, source_id, task_id, run_id, title, content,
              source_uri, metadata_json, created_at, updated_at)
             SELECT 'task:' || id, project_id, 'task', id, id, NULL,
                    '任务 · ' || title,
                    title || char(10) || description || char(10) ||
                    '验收标准：' || acceptance_criteria || char(10) ||
                    '验证命令：' || COALESCE(verification_command, ''),
                    'crewvela://projects/' || project_id || '/tasks/' || id,
                    '{{}}', created_at, updated_at
             FROM ma_tasks WHERE project_id = ?1{UPSERT}"
        ),
        params![project_id],
    )?;
    connection.execute(
        &format!(
            "INSERT INTO ma_knowledge_documents
             (id, project_id, source_type, source_id, task_id, run_id, title, content,
              source_uri, metadata_json, created_at, updated_at)
             SELECT 'conversation:' || m.id, m.project_id, 'conversation', m.id, m.task_id, NULL,
                    '对话 · ' || COALESCE(a.name,
                        CASE m.sender_type WHEN 'user' THEN '用户'
                                           WHEN 'coordinator' THEN '总控'
                                           ELSE m.sender_type END),
                    m.body,
                    'crewvela://projects/' || m.project_id || '/messages/' || m.id,
                    '{{}}', m.created_at, m.created_at
             FROM ma_messages m
             LEFT JOIN ma_agents a ON a.id = m.sender_id
             WHERE m.project_id = ?1{UPSERT}"
        ),
        params![project_id],
    )?;
    connection.execute(
        &format!(
            "INSERT INTO ma_knowledge_documents
             (id, project_id, source_type, source_id, task_id, run_id, title, content,
              source_uri, metadata_json, created_at, updated_at)
             SELECT 'run:' || r.id, r.project_id, 'run', r.id, r.task_id, r.id,
                    '运行结果 · ' || t.title || ' · 第 ' || r.attempt || ' 次',
                    '状态：' || r.status || char(10) || COALESCE(r.output_summary, '') ||
                    CASE WHEN r.error IS NULL OR r.error = '' THEN ''
                         ELSE char(10) || '错误：' || r.error END,
                    'crewvela://projects/' || r.project_id || '/runs/' || r.id,
                    '{{}}', r.created_at,
                    COALESCE(r.finished_at, r.started_at, r.last_heartbeat_at, r.created_at)
             FROM ma_runs r
             JOIN ma_tasks t ON t.id = r.task_id
             WHERE r.project_id = ?1
               AND (r.output_summary IS NOT NULL OR r.error IS NOT NULL){UPSERT}"
        ),
        params![project_id],
    )?;
    connection.execute(
        &format!(
            "INSERT INTO ma_knowledge_documents
             (id, project_id, source_type, source_id, task_id, run_id, title, content,
              source_uri, metadata_json, created_at, updated_at)
             SELECT 'artifact:' || a.id, a.project_id, 'artifact', a.id, a.task_id, a.run_id,
                    '交付物 · ' || t.title || ' · ' || a.artifact_type,
                    a.summary || char(10) || a.uri ||
                    CASE WHEN a.content_hash IS NULL OR a.content_hash = '' THEN ''
                         ELSE char(10) || '内容哈希：' || a.content_hash END,
                    a.uri, '{{}}', a.created_at, a.created_at
             FROM ma_artifacts a
             JOIN ma_tasks t ON t.id = a.task_id
             WHERE a.project_id = ?1{UPSERT}"
        ),
        params![project_id],
    )?;
    connection.execute(
        &format!(
            "INSERT INTO ma_knowledge_documents
             (id, project_id, source_type, source_id, task_id, run_id, title, content,
              source_uri, metadata_json, created_at, updated_at)
             SELECT 'review:' || d.id, d.project_id, 'review', d.id, d.target_task_id,
                    d.review_run_id,
                    '审核 · ' || t.title || ' · ' ||
                    CASE d.verdict WHEN 'approved' THEN '通过' ELSE '要求修改' END,
                    d.summary,
                    'crewvela://projects/' || d.project_id || '/reviews/' || d.id,
                    '{{}}', d.created_at, COALESCE(d.applied_at, d.created_at)
             FROM ma_review_decisions d
             JOIN ma_tasks t ON t.id = d.target_task_id
             WHERE d.project_id = ?1{UPSERT}"
        ),
        params![project_id],
    )?;
    connection.execute(
        &format!(
            "INSERT INTO ma_knowledge_documents
             (id, project_id, source_type, source_id, task_id, run_id, title, content,
              source_uri, metadata_json, created_at, updated_at)
             SELECT 'memory:' || id, project_id, 'memory', id, source_task_id, source_run_id,
                    '经验 · [' || namespace || '] ' || key,
                    value,
                    'crewvela://projects/' || project_id || '/memories/' || id,
                    '{{}}', created_at, updated_at
             FROM ma_memory_entries
             WHERE project_id = ?1 AND verified = 1{UPSERT}"
        ),
        params![project_id],
    )?;

    connection.execute(
        "DELETE FROM ma_knowledge_documents AS d
         WHERE d.project_id = ?1 AND (
            (d.source_type = 'project' AND NOT EXISTS(
                SELECT 1 FROM ma_projects p WHERE p.id = d.source_id
            )) OR
            (d.source_type = 'task' AND NOT EXISTS(
                SELECT 1 FROM ma_tasks t WHERE t.id = d.source_id AND t.project_id = ?1
            )) OR
            (d.source_type = 'conversation' AND NOT EXISTS(
                SELECT 1 FROM ma_messages m WHERE m.id = d.source_id AND m.project_id = ?1
            )) OR
            (d.source_type = 'run' AND NOT EXISTS(
                SELECT 1 FROM ma_runs r WHERE r.id = d.source_id AND r.project_id = ?1
                  AND (r.output_summary IS NOT NULL OR r.error IS NOT NULL)
            )) OR
            (d.source_type = 'artifact' AND NOT EXISTS(
                SELECT 1 FROM ma_artifacts a WHERE a.id = d.source_id AND a.project_id = ?1
            )) OR
            (d.source_type = 'review' AND NOT EXISTS(
                SELECT 1 FROM ma_review_decisions r WHERE r.id = d.source_id AND r.project_id = ?1
            )) OR
            (d.source_type = 'memory' AND NOT EXISTS(
                SELECT 1 FROM ma_memory_entries m WHERE m.id = d.source_id
                  AND m.project_id = ?1 AND m.verified = 1
            ))
         )",
        params![project_id],
    )?;
    Ok(())
}

fn normalize_knowledge_source_types(values: &[String]) -> Vec<String> {
    const ALLOWED: &[&str] = &[
        "project",
        "task",
        "conversation",
        "run",
        "artifact",
        "review",
        "memory",
    ];
    let mut result = values
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| ALLOWED.contains(&value.as_str()))
        .collect::<Vec<_>>();
    result.sort();
    result.dedup();
    result
}

fn build_knowledge_fts_query(query: &str) -> Option<String> {
    let terms = query
        .split_whitespace()
        .map(|value| value.trim_matches(|character: char| character.is_ascii_punctuation()))
        .filter(|value| value.chars().count() >= 3)
        .take(12)
        .map(|value| format!("\"{}\"", value.replace('"', "\"\"")))
        .collect::<Vec<_>>();
    (!terms.is_empty()).then(|| terms.join(" AND "))
}

fn escape_like(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn knowledge_search_result_from_row(row: &Row<'_>) -> rusqlite::Result<KnowledgeSearchResult> {
    Ok(KnowledgeSearchResult {
        id: row.get(0)?,
        project_id: row.get(1)?,
        source_type: row.get(2)?,
        source_id: row.get(3)?,
        task_id: row.get(4)?,
        run_id: row.get(5)?,
        title: row.get(6)?,
        snippet: row.get(7)?,
        content: row.get(8)?,
        source_uri: row.get(9)?,
        relevance: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn required<'a>(value: &'a str, label: &str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        Err(anyhow!("{label}不能为空"))
    } else {
        Ok(value)
    }
}

fn normalize_capabilities(values: &[String]) -> Vec<String> {
    let mut values = values
        .iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn parse_string_list(value: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(value)
        .map(|values| normalize_capabilities(&values))
        .unwrap_or_default()
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn ensure_project_exists(connection: &Connection, project_id: &str) -> Result<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM ma_projects WHERE id = ?1)",
        params![project_id],
        |row| row.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(anyhow!("未找到 Multi-Agent 项目: {project_id}"))
    }
}

fn agent_satisfies_task(agent: &AgentProfile, task: &MultiAgentTask) -> bool {
    task.required_capabilities.iter().all(|required| {
        agent
            .capabilities
            .iter()
            .any(|capability| capability.eq_ignore_ascii_case(required))
    })
}

fn agent_is_manually_controlled(
    connection: &Connection,
    project_id: &str,
    agent_id: &str,
) -> Result<bool> {
    connection
        .query_row(
            "SELECT EXISTS(
                 SELECT 1 FROM ma_agent_sessions
                 WHERE project_id = ?1 AND agent_id = ?2 AND control_mode = 'manual'
             )",
            params![project_id, agent_id],
            |row| row.get(0),
        )
        .map_err(Into::into)
}

fn ensure_agent_in_project(
    connection: &Connection,
    agent_id: &str,
    project_id: &str,
) -> Result<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM ma_agents WHERE id = ?1 AND project_id = ?2)",
        params![agent_id, project_id],
        |row| row.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(anyhow!("Agent 不属于当前项目: {agent_id}"))
    }
}

fn ensure_current_agent_in_project(connection: &Connection, agent_id: &str, project_id: &str) -> Result<()> {
    ensure_agent_in_project(connection, agent_id, project_id)?;
    if get_agent_with(connection, agent_id)?.removed_at.is_some() {
        return Err(anyhow!("该成员已从团队移除"));
    }
    Ok(())
}

fn ensure_task_in_project(connection: &Connection, task_id: &str, project_id: &str) -> Result<()> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM ma_tasks WHERE id = ?1 AND project_id = ?2)",
        params![task_id, project_id],
        |row| row.get(0),
    )?;
    if exists {
        Ok(())
    } else {
        Err(anyhow!("任务不属于当前项目: {task_id}"))
    }
}

fn task_project_id(connection: &Connection, task_id: &str) -> Result<String> {
    connection
        .query_row(
            "SELECT project_id FROM ma_tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("未找到任务: {task_id}"))
}

fn dependency_reaches(connection: &Connection, start: &str, target: &str) -> Result<bool> {
    Ok(connection.query_row(
        "WITH RECURSIVE deps(id) AS (
            SELECT depends_on_task_id FROM ma_task_dependencies WHERE task_id = ?1
            UNION
            SELECT d.depends_on_task_id FROM ma_task_dependencies d JOIN deps ON d.task_id = deps.id
         ) SELECT EXISTS(SELECT 1 FROM deps WHERE id = ?2)",
        params![start, target],
        |row| row.get(0),
    )?)
}

struct PlannedWorkerDelivery {
    artifact_id: String,
    run_id: String,
    workspace: String,
    hash: String,
    scopes: Vec<String>,
}

fn planned_worker_delivery(connection: &Connection, task: &MultiAgentTask) -> Result<Option<PlannedWorkerDelivery>> {
    if task.workspace_mode != WorkspaceMode::Worktree {
        return Ok(None);
    }
    let Some(agent_id) = task.assignee_agent_id.as_deref() else { return Ok(None) };
    let raw: Option<String> = connection.query_row(
        "SELECT s.write_paths_json FROM ma_conversation_task_scopes s
         JOIN ma_agents a ON a.id=?2 AND a.project_id=?3 AND a.role='worker'
         WHERE s.task_id=?1",
        params![task.id, agent_id, task.project_id],
        |row| row.get(0),
    ).optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let scopes: Vec<String> = serde_json::from_str(&raw)?;
    if scopes.is_empty() {
        return Ok(None);
    }
    let (artifact_id, run_id, workspace, hash) = connection.query_row(
        "SELECT a.id,r.id,a.uri,a.content_hash FROM ma_runs r
         JOIN ma_artifacts a ON a.run_id=r.id AND a.task_id=r.task_id AND a.artifact_type='git_commit'
         WHERE r.task_id=?1 AND r.agent_id=?2 AND r.status='succeeded'
         AND r.attempt=(SELECT MAX(attempt) FROM ma_runs WHERE task_id=?1)
         AND EXISTS(SELECT 1 FROM ma_events e WHERE e.project_id=r.project_id AND e.entity_id=r.task_id
             AND e.event_type='task.execution_completed' AND json_extract(e.payload_json,'$.artifactId')=a.id)
         ORDER BY a.created_at DESC,a.rowid DESC LIMIT 1",
        params![task.id, agent_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get::<_, Option<String>>(3)?)),
    ).optional()?.context("任务没有可核验的原工作树交付")?;
    let hash = hash.context("任务缺少交付提交哈希")?;
    Ok(Some(PlannedWorkerDelivery { artifact_id, run_id, workspace, hash, scopes }))
}

fn unblock_ready_tasks(connection: &Connection, project_id: &str, now: i64) -> Result<usize> {
    Ok(connection.execute(
        "UPDATE ma_tasks SET status = 'ready', updated_at = ?2
         WHERE project_id = ?1 AND status = 'blocked'
         AND NOT EXISTS (
             SELECT 1 FROM ma_task_dependencies d
             JOIN ma_tasks dependency ON dependency.id = d.depends_on_task_id
             WHERE d.task_id = ma_tasks.id AND dependency.status != 'done'
         )",
        params![project_id, now],
    )?)
}

fn append_event(
    connection: &Connection,
    project_id: &str,
    entity_type: &str,
    entity_id: &str,
    event_type: &str,
    actor_type: &str,
    actor_id: Option<&str>,
    payload: serde_json::Value,
    created_at: i64,
) -> Result<()> {
    connection.execute(
        "INSERT INTO ma_events
         (project_id, entity_type, entity_id, event_type, actor_type, actor_id, payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![project_id, entity_type, entity_id, event_type, actor_type, actor_id, payload.to_string(), created_at],
    )?;
    Ok(())
}

fn validate_run_transition(from: RunStatus, to: RunStatus) -> Result<()> {
    let allowed = match from {
        RunStatus::Queued => matches!(to, RunStatus::Starting | RunStatus::Cancelled),
        RunStatus::Starting => matches!(
            to,
            RunStatus::Running | RunStatus::Failed | RunStatus::Cancelled | RunStatus::Interrupted
        ),
        RunStatus::Running => matches!(
            to,
            RunStatus::WaitingInput
                | RunStatus::WaitingApproval
                | RunStatus::Succeeded
                | RunStatus::Failed
                | RunStatus::TimedOut
                | RunStatus::Cancelled
                | RunStatus::Interrupted
        ),
        RunStatus::WaitingInput | RunStatus::WaitingApproval => matches!(
            to,
            RunStatus::Running
                | RunStatus::Succeeded
                | RunStatus::Failed
                | RunStatus::TimedOut
                | RunStatus::Cancelled
                | RunStatus::Interrupted
        ),
        RunStatus::Succeeded
        | RunStatus::Failed
        | RunStatus::TimedOut
        | RunStatus::Cancelled
        | RunStatus::Interrupted => false,
    };
    if allowed {
        Ok(())
    } else {
        Err(anyhow!(
            "非法 Run 状态转换: {} -> {}",
            from.as_str(),
            to.as_str()
        ))
    }
}

fn is_terminal_run_status(status: RunStatus) -> bool {
    matches!(
        status,
        RunStatus::Succeeded
            | RunStatus::Failed
            | RunStatus::TimedOut
            | RunStatus::Cancelled
            | RunStatus::Interrupted
    )
}

fn task_status_for_run(status: RunStatus) -> TaskStatus {
    match status {
        RunStatus::Queued | RunStatus::Starting => TaskStatus::Claimed,
        RunStatus::Running | RunStatus::WaitingInput => TaskStatus::Running,
        RunStatus::WaitingApproval => TaskStatus::WaitingApproval,
        RunStatus::Succeeded => TaskStatus::InReview,
        RunStatus::Failed | RunStatus::TimedOut | RunStatus::Interrupted => TaskStatus::Failed,
        RunStatus::Cancelled => TaskStatus::Cancelled,
    }
}

fn project_from_row(row: &Row<'_>) -> rusqlite::Result<MultiAgentProject> {
    Ok(MultiAgentProject {
        id: row.get(0)?,
        name: row.get(1)?,
        goal: row.get(2)?,
        root_path: row.get(3)?,
        default_branch: row.get(4)?,
        status: parse_project_status(row.get::<_, String>(5)?)?,
        created_at: row.get(6)?,
        updated_at: row.get(7)?,
    })
}

/// persona_custom 只在人格为 custom 时保留（截断到 120 字），其余人格清空，
/// 避免切换回内置人格后残留的自由文本继续进入派单提示词。
fn sanitized_persona_custom(persona: &AgentPersona, custom: &str) -> String {
    if *persona != AgentPersona::Custom {
        return String::new();
    }
    let trimmed = custom.trim();
    trimmed.chars().take(120).collect()
}

fn agent_from_row(row: &Row<'_>) -> rusqlite::Result<AgentProfile> {
    let capabilities: String = row.get(12)?;
    let persona: String = row.get(14)?;
    Ok(AgentProfile {
        avatar_id: row.get(13)?,
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        runtime_kind: parse_runtime_kind(row.get::<_, String>(3)?)?,
        role: parse_agent_role(row.get::<_, String>(4)?)?,
        description: row.get(5)?,
        persona: super::domain::AgentPersona::parse(&persona),
        persona_custom: row.get(15)?,
        model: row.get(6)?,
        permission_profile: parse_permission_profile(row.get::<_, String>(7)?)?,
        max_concurrency: row.get(8)?,
        enabled: row.get(9)?,
        removed_at: row.get(16)?,
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
        capabilities: parse_string_list(&capabilities),
    })
}

fn agent_from_row_offset(row: &Row<'_>, offset: usize) -> rusqlite::Result<AgentProfile> {
    let capabilities: String = row.get(offset + 12)?;
    let persona: String = row.get(offset + 14)?;
    Ok(AgentProfile {
        avatar_id: row.get(offset + 13)?,
        id: row.get(offset)?,
        project_id: row.get(offset + 1)?,
        name: row.get(offset + 2)?,
        runtime_kind: parse_runtime_kind(row.get::<_, String>(offset + 3)?)?,
        role: parse_agent_role(row.get::<_, String>(offset + 4)?)?,
        description: row.get(offset + 5)?,
        persona: super::domain::AgentPersona::parse(&persona),
        persona_custom: row.get(offset + 15)?,
        model: row.get(offset + 6)?,
        permission_profile: parse_permission_profile(row.get::<_, String>(offset + 7)?)?,
        max_concurrency: row.get(offset + 8)?,
        enabled: row.get(offset + 9)?,
        removed_at: row.get(offset + 16)?,
        created_at: row.get(offset + 10)?,
        updated_at: row.get(offset + 11)?,
        capabilities: parse_string_list(&capabilities),
    })
}

fn task_from_row(row: &Row<'_>) -> rusqlite::Result<MultiAgentTask> {
    let required_capabilities: String = row.get(15)?;
    Ok(MultiAgentTask {
        id: row.get(0)?,
        project_id: row.get(1)?,
        parent_id: row.get(2)?,
        stage: row.get(3)?,
        title: row.get(4)?,
        description: row.get(5)?,
        acceptance_criteria: row.get(6)?,
        verification_command: row.get(7)?,
        status: parse_task_status(row.get::<_, String>(8)?)?,
        assignee_agent_id: row.get(9)?,
        reviewer_agent_id: row.get(10)?,
        workspace_mode: parse_workspace_mode(row.get::<_, String>(11)?)?,
        priority: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
        required_capabilities: parse_string_list(&required_capabilities),
        max_attempts: row.get(16)?,
        retry_backoff_seconds: row.get::<_, i64>(17)?.max(0) as u64,
        next_retry_at: row.get(18)?,
    })
}

fn run_from_row(row: &Row<'_>) -> rusqlite::Result<AgentRun> {
    let locator: Option<String> = row.get(6)?;
    Ok(AgentRun {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_id: row.get(2)?,
        agent_id: row.get(3)?,
        attempt: row.get(4)?,
        status: parse_run_status(row.get::<_, String>(5)?)?,
        session_locator: locator.and_then(|value| serde_json::from_str(&value).ok()),
        lease_expires_at: row.get(7)?,
        output_summary: row.get(8)?,
        error: row.get(9)?,
        created_at: row.get(10)?,
        started_at: row.get(11)?,
        finished_at: row.get(12)?,
        last_heartbeat_at: row.get(13)?,
    })
}

fn wakeup_from_row(row: &Row<'_>) -> rusqlite::Result<WakeupRequest> {
    Ok(WakeupRequest {
        id: row.get(0)?,
        project_id: row.get(1)?,
        agent_id: row.get(2)?,
        task_id: row.get(3)?,
        run_id: row.get(4)?,
        reason: row.get(5)?,
        status: row.get(6)?,
        idempotency_key: row.get(7)?,
        coalesced_count: row.get(8)?,
        requested_at: row.get(9)?,
    })
}

fn event_from_row(row: &Row<'_>) -> rusqlite::Result<ActivityEvent> {
    let payload: String = row.get(7)?;
    Ok(ActivityEvent {
        seq: row.get(0)?,
        project_id: row.get(1)?,
        entity_type: row.get(2)?,
        entity_id: row.get(3)?,
        event_type: row.get(4)?,
        actor_type: row.get(5)?,
        actor_id: row.get(6)?,
        payload: serde_json::from_str(&payload)
            .unwrap_or_else(|_| serde_json::json!({ "raw": payload })),
        created_at: row.get(8)?,
    })
}

pub(super) fn runtime_event_from_row(row: &Row<'_>) -> rusqlite::Result<RuntimeEventRecord> {
    let payload: String = row.get(6)?;
    Ok(RuntimeEventRecord {
        seq: row.get(0)?,
        project_id: row.get(1)?,
        run_id: row.get(2)?,
        task_id: row.get(3)?,
        agent_id: row.get(4)?,
        event_type: row.get(5)?,
        payload: serde_json::from_str(&payload)
            .unwrap_or_else(|_| serde_json::json!({"raw": payload})),
        created_at: row.get(7)?,
    })
}

fn memory_from_row(row: &Row<'_>) -> rusqlite::Result<MemoryEntry> {
    Ok(MemoryEntry {
        id: row.get(0)?,
        project_id: row.get(1)?,
        namespace: row.get(2)?,
        key: row.get(3)?,
        value: row.get(4)?,
        source_task_id: row.get(5)?,
        source_run_id: row.get(6)?,
        confidence: row.get(7)?,
        verified: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn review_decision_from_row(row: &Row<'_>) -> rusqlite::Result<ReviewDecision> {
    Ok(ReviewDecision {
        id: row.get(0)?,
        project_id: row.get(1)?,
        review_task_id: row.get(2)?,
        review_run_id: row.get(3)?,
        reviewer_agent_id: row.get(4)?,
        target_task_id: row.get(5)?,
        verdict: parse_review_verdict(row.get::<_, String>(6)?)?,
        summary: row.get(7)?,
        created_at: row.get(8)?,
        applied_at: row.get(9)?,
    })
}

fn collaboration_message_from_row(row: &Row<'_>) -> rusqlite::Result<CollaborationMessage> {
    Ok(CollaborationMessage {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_id: row.get(2)?,
        sender_type: row.get(3)?,
        sender_id: row.get(4)?,
        target_agent_id: row.get(5)?,
        body: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn runtime_event_by_seq(connection: &Connection, seq: i64) -> Result<RuntimeEventRecord> {
    connection
        .query_row(
            "SELECT seq, project_id, run_id, task_id, agent_id, event_type,
                    payload_json, created_at
             FROM ma_runtime_events WHERE seq = ?1",
            params![seq],
            runtime_event_from_row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("未找到运行事件: {seq}"))
}

fn get_agent_with(connection: &Connection, id: &str) -> Result<AgentProfile> {
    connection
        .query_row(
            "SELECT id, project_id, name, runtime_kind, role, description, model,
         permission_profile, max_concurrency, enabled, created_at, updated_at, capabilities_json, avatar_id,
         persona, persona_custom, removed_at
         FROM ma_agents WHERE id = ?1",
            params![id],
            agent_from_row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("未找到 Agent: {id}"))
}

fn get_task_with(connection: &Connection, id: &str) -> Result<MultiAgentTask> {
    connection
        .query_row(
            "SELECT id, project_id, parent_id, stage, title, description, acceptance_criteria,
         verification_command, status, assignee_agent_id, reviewer_agent_id, workspace_mode,
         priority, created_at, updated_at, required_capabilities_json, max_attempts,
         retry_backoff_seconds, next_retry_at
         FROM ma_tasks WHERE id = ?1",
            params![id],
            task_from_row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("未找到任务: {id}"))
}

fn get_run_with(connection: &Connection, id: &str) -> Result<AgentRun> {
    connection
        .query_row(
            "SELECT id, project_id, task_id, agent_id, attempt, status, session_locator_json,
         lease_expires_at, output_summary, error, created_at, started_at, finished_at,
         last_heartbeat_at
         FROM ma_runs WHERE id = ?1",
            params![id],
            run_from_row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("未找到 Run: {id}"))
}

fn get_wakeup_with(connection: &Connection, id: &str) -> Result<WakeupRequest> {
    connection
        .query_row(
            "SELECT id, project_id, agent_id, task_id, run_id, reason, status,
         idempotency_key, coalesced_count, requested_at FROM ma_wakeups WHERE id = ?1",
            params![id],
            wakeup_from_row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("未找到 Wakeup: {id}"))
}

fn list_agents_with(connection: &Connection, project_id: &str) -> Result<Vec<AgentProfile>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, name, runtime_kind, role, description, model,
         permission_profile, max_concurrency, enabled, created_at, updated_at, capabilities_json, avatar_id,
         persona, persona_custom, removed_at
         FROM ma_agents WHERE project_id = ?1 ORDER BY created_at",
    )?;
    let agents = statement
        .query_map(params![project_id], agent_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(agents)
}

fn list_tasks_with(connection: &Connection, project_id: &str) -> Result<Vec<MultiAgentTask>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, parent_id, stage, title, description, acceptance_criteria,
         verification_command, status, assignee_agent_id, reviewer_agent_id, workspace_mode,
         priority, created_at, updated_at, required_capabilities_json, max_attempts,
         retry_backoff_seconds, next_retry_at
         FROM ma_tasks WHERE project_id = ?1 ORDER BY stage, priority DESC, created_at",
    )?;
    let tasks = statement
        .query_map(params![project_id], task_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(tasks)
}

fn list_runs_with(connection: &Connection, project_id: &str) -> Result<Vec<AgentRun>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, task_id, agent_id, attempt, status, session_locator_json,
         lease_expires_at, output_summary, error, created_at, started_at, finished_at,
         last_heartbeat_at
         FROM ma_runs WHERE project_id = ?1 ORDER BY created_at DESC",
    )?;
    let runs = statement
        .query_map(params![project_id], run_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(runs)
}

fn list_wakeups_with(connection: &Connection, project_id: &str) -> Result<Vec<WakeupRequest>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, agent_id, task_id, run_id, reason, status,
         idempotency_key, coalesced_count, requested_at
         FROM ma_wakeups WHERE project_id = ?1 ORDER BY requested_at DESC LIMIT 200",
    )?;
    let wakeups = statement
        .query_map(params![project_id], wakeup_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(wakeups)
}

fn list_events_with(
    connection: &Connection,
    project_id: &str,
    limit: u32,
) -> Result<Vec<ActivityEvent>> {
    let mut statement = connection.prepare(
        "SELECT seq, project_id, entity_type, entity_id, event_type, actor_type, actor_id,
         payload_json, created_at FROM ma_events WHERE project_id = ?1
         ORDER BY seq DESC LIMIT ?2",
    )?;
    let mut events = statement
        .query_map(params![project_id, limit], event_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    events.reverse();
    Ok(events)
}

fn list_runtime_events_with(
    connection: &Connection,
    project_id: &str,
    after_seq: Option<i64>,
    limit: u32,
) -> Result<Vec<RuntimeEventRecord>> {
    if let Some(after_seq) = after_seq.filter(|value| *value > 0) {
        let mut statement = connection.prepare(
            "SELECT seq, project_id, run_id, task_id, agent_id, event_type,
                    payload_json, created_at
             FROM ma_runtime_events
             WHERE project_id = ?1 AND seq > ?2
             ORDER BY seq LIMIT ?3",
        )?;
        return Ok(statement
            .query_map(
                params![project_id, after_seq, limit],
                runtime_event_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?);
    }
    let mut statement = connection.prepare(
        "SELECT seq, project_id, run_id, task_id, agent_id, event_type,
                payload_json, created_at
         FROM ma_runtime_events
         WHERE project_id = ?1
         ORDER BY seq DESC LIMIT ?2",
    )?;
    let mut events = statement
        .query_map(params![project_id, limit], runtime_event_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    events.reverse();
    Ok(events)
}

fn list_recent_memories_with(
    connection: &Connection,
    project_id: &str,
    limit: u32,
) -> Result<Vec<MemoryEntry>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, namespace, key, value, source_task_id, source_run_id,
                confidence, verified, created_at, updated_at
         FROM ma_memory_entries
         WHERE project_id = ?1 AND verified = 1
         ORDER BY updated_at DESC LIMIT ?2",
    )?;
    let memories = statement
        .query_map(params![project_id, limit], memory_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(memories)
}

fn list_review_decisions_with(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<ReviewDecision>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, review_task_id, review_run_id, reviewer_agent_id,
                target_task_id, verdict, summary, created_at, applied_at
         FROM ma_review_decisions WHERE project_id = ?1 ORDER BY created_at",
    )?;
    let decisions = statement
        .query_map(params![project_id], review_decision_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(decisions)
}

fn list_messages_with(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<CollaborationMessage>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, task_id, sender_type, sender_id, target_agent_id, body, created_at
         FROM ma_messages WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 200",
    )?;
    let mut items = statement
        .query_map(params![project_id], |row| {
            collaboration_message_from_row(row)
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    items.reverse();
    Ok(items)
}

fn list_artifacts_with(connection: &Connection, project_id: &str) -> Result<Vec<ArtifactRecord>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, task_id, run_id, artifact_type, uri, summary, content_hash, created_at
         FROM ma_artifacts WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 200",
    )?;
    let items = statement
        .query_map(params![project_id], |row| {
            Ok(ArtifactRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                task_id: row.get(2)?,
                run_id: row.get(3)?,
                artifact_type: row.get(4)?,
                uri: row.get(5)?,
                summary: row.get(6)?,
                content_hash: row.get(7)?,
                created_at: row.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(items)
}

fn approval_from_row(row: &Row<'_>) -> rusqlite::Result<ApprovalRecord> {
    Ok(ApprovalRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        task_id: row.get(2)?,
        run_id: row.get(3)?,
        approval_type: row.get(4)?,
        summary: row.get(5)?,
        status: row.get(6)?,
        created_at: row.get(7)?,
        decided_at: row.get(8)?,
    })
}

fn list_approvals_with(connection: &Connection, project_id: &str) -> Result<Vec<ApprovalRecord>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, task_id, run_id, approval_type, summary, status, created_at, decided_at
         FROM ma_approvals WHERE project_id = ?1 ORDER BY created_at DESC LIMIT 200",
    )?;
    let items = statement
        .query_map(params![project_id], approval_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(items)
}

fn list_dependencies_with(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<TaskDependencyRecord>> {
    let mut statement = connection.prepare(
        "SELECT dependency.task_id, dependency.depends_on_task_id
         FROM ma_task_dependencies dependency
         INNER JOIN ma_tasks task ON task.id = dependency.task_id
         WHERE task.project_id = ?1
         ORDER BY task.stage, dependency.task_id, dependency.depends_on_task_id",
    )?;
    let items = statement
        .query_map(params![project_id], |row| {
            Ok(TaskDependencyRecord {
                task_id: row.get(0)?,
                depends_on_task_id: row.get(1)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(items)
}

fn list_agent_sessions_with(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<AgentSessionRecord>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, agent_id, current_task_id, current_run_id, app_session_id, status,
         control_mode, last_output_at, created_at, updated_at
         FROM ma_agent_sessions WHERE project_id = ?1 ORDER BY created_at",
    )?;
    let items = statement
        .query_map(params![project_id], |row| {
            Ok(AgentSessionRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                agent_id: row.get(2)?,
                current_task_id: row.get(3)?,
                current_run_id: row.get(4)?,
                app_session_id: row.get(5)?,
                status: parse_agent_session_status(row.get(6)?)?,
                control_mode: parse_agent_control_mode(row.get(7)?)?,
                last_output_at: row.get(8)?,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(items)
}

fn list_run_logs_with(
    connection: &Connection,
    project_id: &str,
    limit: u32,
) -> Result<Vec<RunLogEntry>> {
    let mut statement = connection.prepare(
        "SELECT seq, project_id, run_id, task_id, agent_id, stream, content, created_at
         FROM ma_run_logs WHERE project_id = ?1 ORDER BY seq DESC LIMIT ?2",
    )?;
    let mut items = statement
        .query_map(params![project_id, limit], run_log_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    items.reverse();
    Ok(items)
}

fn run_log_from_row(row: &Row<'_>) -> rusqlite::Result<RunLogEntry> {
    Ok(RunLogEntry {
        seq: row.get(0)?,
        project_id: row.get(1)?,
        run_id: row.get(2)?,
        task_id: row.get(3)?,
        agent_id: row.get(4)?,
        stream: row.get(5)?,
        content: row.get(6)?,
        created_at: row.get(7)?,
    })
}

fn list_instructions_with(
    connection: &Connection,
    project_id: &str,
) -> Result<Vec<AgentInstruction>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, agent_id, task_id, body, status, created_at, delivered_at
         FROM ma_agent_instructions WHERE project_id = ?1
         ORDER BY created_at DESC LIMIT 200",
    )?;
    let items = statement
        .query_map(params![project_id], |row| {
            Ok(AgentInstruction {
                id: row.get(0)?,
                project_id: row.get(1)?,
                agent_id: row.get(2)?,
                task_id: row.get(3)?,
                body: row.get(4)?,
                status: row.get(5)?,
                created_at: row.get(6)?,
                delivered_at: row.get(7)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(items)
}

fn parse_project_status(value: String) -> rusqlite::Result<ProjectStatus> {
    parse_enum(value, |value| match value {
        "draft" => Some(ProjectStatus::Draft),
        "active" => Some(ProjectStatus::Active),
        "paused" => Some(ProjectStatus::Paused),
        "completed" => Some(ProjectStatus::Completed),
        "cancelled" => Some(ProjectStatus::Cancelled),
        _ => None,
    })
}

fn parse_runtime_kind(value: String) -> rusqlite::Result<AgentRuntimeKind> {
    parse_enum(value, |value| match value {
        "claude_code" => Some(AgentRuntimeKind::ClaudeCode),
        "codex" => Some(AgentRuntimeKind::Codex),
        "kimi_code" => Some(AgentRuntimeKind::KimiCode),
        "minimax_code" => Some(AgentRuntimeKind::MiniMaxCode),
        "pi" => Some(AgentRuntimeKind::Pi),
        "oh_my_pi" => Some(AgentRuntimeKind::OhMyPi),
        "opencode" => Some(AgentRuntimeKind::OpenCode),
        "deepseek_harness" => Some(AgentRuntimeKind::DeepSeekHarness),
        "zcode" => Some(AgentRuntimeKind::ZCode),
        "zcode_cli" => Some(AgentRuntimeKind::ZCodeCli),
        "antigravity" => Some(AgentRuntimeKind::Antigravity),
        _ => Some(AgentRuntimeKind::Unsupported),
    })
}

fn parse_agent_role(value: String) -> rusqlite::Result<AgentRole> {
    parse_enum(value, |value| match value {
        "orchestrator" => Some(AgentRole::Orchestrator),
        "worker" => Some(AgentRole::Worker),
        "reviewer" => Some(AgentRole::Reviewer),
        "integrator" => Some(AgentRole::Integrator),
        _ => None,
    })
}

fn parse_permission_profile(value: String) -> rusqlite::Result<PermissionProfile> {
    parse_enum(value, |value| match value {
        "default" => Some(PermissionProfile::Default),
        "auto" => Some(PermissionProfile::Auto),
        "yolo" => Some(PermissionProfile::Yolo),
        _ => None,
    })
}

fn parse_task_status(value: String) -> rusqlite::Result<TaskStatus> {
    parse_enum(value, |value| match value {
        "draft" => Some(TaskStatus::Draft),
        "ready" => Some(TaskStatus::Ready),
        "blocked" => Some(TaskStatus::Blocked),
        "claimed" => Some(TaskStatus::Claimed),
        "running" => Some(TaskStatus::Running),
        "waiting_approval" => Some(TaskStatus::WaitingApproval),
        "in_review" => Some(TaskStatus::InReview),
        "changes_requested" => Some(TaskStatus::ChangesRequested),
        "done" => Some(TaskStatus::Done),
        "failed" => Some(TaskStatus::Failed),
        "cancelled" => Some(TaskStatus::Cancelled),
        _ => None,
    })
}

fn parse_run_status(value: String) -> rusqlite::Result<RunStatus> {
    parse_enum(value, |value| match value {
        "queued" => Some(RunStatus::Queued),
        "starting" => Some(RunStatus::Starting),
        "running" => Some(RunStatus::Running),
        "waiting_input" => Some(RunStatus::WaitingInput),
        "waiting_approval" => Some(RunStatus::WaitingApproval),
        "succeeded" => Some(RunStatus::Succeeded),
        "failed" => Some(RunStatus::Failed),
        "timed_out" => Some(RunStatus::TimedOut),
        "cancelled" => Some(RunStatus::Cancelled),
        "interrupted" => Some(RunStatus::Interrupted),
        _ => None,
    })
}

fn parse_review_verdict(value: String) -> rusqlite::Result<ReviewVerdict> {
    parse_enum(value, |value| match value {
        "approved" => Some(ReviewVerdict::Approved),
        "changes_requested" => Some(ReviewVerdict::ChangesRequested),
        _ => None,
    })
}

fn parse_agent_session_status(value: String) -> rusqlite::Result<AgentSessionStatus> {
    parse_enum(value, |value| match value {
        "idle" => Some(AgentSessionStatus::Idle),
        "starting" => Some(AgentSessionStatus::Starting),
        "running" => Some(AgentSessionStatus::Running),
        "waiting" => Some(AgentSessionStatus::Waiting),
        "completed" => Some(AgentSessionStatus::Completed),
        "failed" => Some(AgentSessionStatus::Failed),
        "stopped" => Some(AgentSessionStatus::Stopped),
        _ => None,
    })
}

fn parse_agent_control_mode(value: String) -> rusqlite::Result<AgentControlMode> {
    parse_enum(value, |value| match value {
        "auto" => Some(AgentControlMode::Auto),
        "manual" => Some(AgentControlMode::Manual),
        _ => None,
    })
}

fn session_status_for_run(status: RunStatus) -> AgentSessionStatus {
    match status {
        RunStatus::Queued | RunStatus::Starting => AgentSessionStatus::Starting,
        RunStatus::Running => AgentSessionStatus::Running,
        RunStatus::WaitingInput | RunStatus::WaitingApproval => AgentSessionStatus::Waiting,
        RunStatus::Succeeded => AgentSessionStatus::Completed,
        RunStatus::Failed | RunStatus::TimedOut => AgentSessionStatus::Failed,
        RunStatus::Cancelled | RunStatus::Interrupted => AgentSessionStatus::Stopped,
    }
}

fn parse_workspace_mode(value: String) -> rusqlite::Result<WorkspaceMode> {
    parse_enum(value, |value| match value {
        "read_only" => Some(WorkspaceMode::ReadOnly),
        "worktree" => Some(WorkspaceMode::Worktree),
        "shared_write" => Some(WorkspaceMode::SharedWrite),
        _ => None,
    })
}

fn parse_enum<T, F>(value: String, parser: F) -> rusqlite::Result<T>
where
    F: FnOnce(&str) -> Option<T>,
{
    parser(&value).ok_or_else(|| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("未知枚举值: {value}"),
            )),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestStore {
        store: Option<MultiAgentStore>,
        root: PathBuf,
    }

    impl TestStore {
        fn new() -> Self {
            let root = std::env::temp_dir().join(format!("crewvela-ma-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let store = MultiAgentStore::open_at(root.join("control.db")).unwrap();
            Self {
                store: Some(store),
                root,
            }
        }

        fn store(&self) -> &MultiAgentStore {
            self.store.as_ref().unwrap()
        }

        fn project(&self) -> MultiAgentProject {
            self.store()
                .create_project(CreateProjectInput {
                    name: "测试项目".to_string(),
                    goal: "验证控制面".to_string(),
                    root_path: self.root.display().to_string(),
                    default_branch: "main".to_string(),
                })
                .unwrap()
        }

        fn agent(&self, project_id: &str, name: &str) -> AgentProfile {
            self.store()
                .create_agent(CreateAgentInput {
                    avatar_id: None,
                    project_id: project_id.to_string(),
                    name: name.to_string(),
                    runtime_kind: AgentRuntimeKind::Codex,
                    role: AgentRole::Worker,
                    description: "执行测试任务".to_string(),
                    model: None,
                    permission_profile: PermissionProfile::Auto,
                    max_concurrency: 1,
                    persona: Default::default(),
                    persona_custom: String::new(),
                })
                .unwrap()
        }

        fn task(&self, project_id: &str, agent_id: &str, title: &str) -> MultiAgentTask {
            self.store()
                .create_task(CreateTaskInput {
                    project_id: project_id.to_string(),
                    parent_id: None,
                    stage: 0,
                    title: title.to_string(),
                    description: String::new(),
                    acceptance_criteria: "测试通过".to_string(),
                    verification_command: None,
                    assignee_agent_id: Some(agent_id.to_string()),
                    reviewer_agent_id: None,
                    workspace_mode: WorkspaceMode::Worktree,
                    priority: 0,
                    required_capabilities: Vec::new(),
                    max_attempts: 3,
                    retry_backoff_seconds: 5,
                })
                .unwrap()
        }
    }

    impl Drop for TestStore {
        fn drop(&mut self) {
            self.store.take();
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn project_activity_keeps_approvals_and_running_rounds_scoped_by_project() -> Result<()> {
        let fixture = TestStore::new();
        let store = fixture.store();
        let waiting = fixture.project();
        let working = fixture.project();
        let delivered = fixture.project();
        let inactive = fixture.project();
        {
            let db = store.connection.lock().unwrap();
            for (id, project, status) in [
                ("waiting", &waiting.id, "waiting_approval"),
                ("working", &working.id, "running"),
                ("delivered", &delivered.id, "completed"),
            ] {
                db.execute(
                    "INSERT INTO ma_conversation_rounds (id,project_id,client_message_id,request_json,status,data_json,created_at)
                     VALUES (?1,?2,?1,'{}',?3,'{}',1)", params![id, project, status],
                )?;
            }
            db.execute("UPDATE ma_projects SET status='completed' WHERE id=?1", [&delivered.id])?;
            db.execute(
                "INSERT INTO ma_conversation_plans (id,project_id,round_id,data_json)
                 VALUES ('delivery-plan',?1,'delivered',?2)",
                params![delivered.id, r#"{"managed":{"status":"completed"}}"#],
            )?;
        }
        let activity = || -> Result<std::collections::HashMap<String, String>> {
            Ok(store.list_project_activity()?.into_iter()
                .map(|item| (item.project.id, item.activity)).collect())
        };
        let initial = activity()?;
        assert_eq!(initial[&waiting.id], "pending_approval");
        assert_eq!(initial[&working.id], "running");
        assert_eq!(initial[&delivered.id], "delivered");
        assert_eq!(initial[&inactive.id], "active");
        store.connection.lock().unwrap().execute(
            "UPDATE ma_conversation_rounds SET status='running' WHERE id='waiting'", [],
        )?;
        let updated = activity()?;
        assert_eq!(updated[&waiting.id], "running");
        assert_eq!(updated[&working.id], "running");
        assert_eq!(updated[&delivered.id], "delivered");
        let renamed = store.rename_project(&delivered.id, " 已验收演示 ")?;
        assert_eq!(renamed.name, "已验收演示");
        assert_eq!(activity()?, updated);
        let saved_plan: String = store.connection.lock().unwrap().query_row(
            "SELECT data_json FROM ma_conversation_plans WHERE id = 'delivery-plan' AND project_id = ?1",
            [&delivered.id], |row| row.get(0),
        )?;
        assert_eq!(saved_plan, r#"{"managed":{"status":"completed"}}"#);
        Ok(())
    }

    #[test]
    fn native_history_keeps_the_registered_ordinary_chat_and_rejects_an_orphan() -> Result<()> {
        use crate::session_switch_history::ConversationRunHistory;
        use serde_json::json;
        let fixture = TestStore::new();
        let project = fixture.project();
        let member = fixture.agent(&project.id, "普通成员");
        let task = fixture.task(&project.id, &member.id, "回复消息");
        let store = fixture.store();
        let task_id = "cli-message-conversation-existing";
        {
            let db = store.connection.lock().unwrap();
            db.execute("UPDATE ma_tasks SET id=?1 WHERE id=?2", params![task_id,task.id])?;
            db.execute("INSERT INTO ma_conversation_rounds VALUES('round',?1,'client','{}','completed','{}',1)", [&project.id])?;
            db.execute("INSERT INTO ma_conversation_turns VALUES('turn','round',?1,?2,0)",
                params![task_id,json!({"agent_id":member.id}).to_string()])?;
        }
        store.set_project_status(&project.id, ProjectStatus::Active)?;
        let run = store.claim_task(ClaimTaskInput{task_id:task_id.into(),agent_id:member.id,lease_seconds:120})?;
        let locator = json!({"adapter":"codex","cwd":fixture.root,"threadId":"ordinary-chat"});
        store.update_run(UpdateRunInput{run_id:run.id.clone(),status:RunStatus::Running,session_locator:Some(locator.clone()),output_summary:None,error:None})?;
        assert!(matches!(store.conversation_run_locator(&project.id,&run.id)?, ConversationRunHistory::Chat(value) if value == locator));
        store.connection.lock().unwrap().execute("DELETE FROM ma_conversation_turns WHERE task_id=?1",[task_id])?;
        assert!(store.conversation_run_locator(&project.id,&run.id).is_err());
        Ok(())
    }

    #[test]
    fn managed_history_requires_applied_project_grant_and_host_identity() -> Result<()> {
        use crate::session_switch_history::ConversationRunHistory;
        use serde_json::json;
        let fixture = TestStore::new();
        let project = fixture.project();
        let member = fixture.agent(&project.id, "history");
        let task = fixture.task(&project.id, &member.id, "原生记录");
        let store = fixture.store();
        store.set_project_status(&project.id, ProjectStatus::Active)?;
        let run = store.claim_task(ClaimTaskInput { task_id: task.id.clone(), agent_id: member.id.clone(), lease_seconds: 120 })?;
        let cwd = &fixture.root;
        let locator = json!({"adapter":"codex", "cwd":cwd});
        store.update_run(UpdateRunInput {run_id:run.id.clone(), status:RunStatus::Running,session_locator:Some(locator),output_summary:None,error:None})?;
        let view = || store.conversation_run_locator(&project.id, &run.id);
        assert!(view().is_err());
        let target = json!({"planner":member.id,"members":{member.id.clone():{"runtime":"codex"}}});
        let plan = json!({"status":"applied","taskIds":[task.id],"managed":{"target":target}});
        {
            let db = store.connection.lock().unwrap();
            db.execute("INSERT INTO ma_conversation_plans(id,project_id,round_id,data_json,applied_request_id) VALUES('p',?1,'r',?2,'ordinary-plan')",
                params![project.id,plan.to_string()])?;
            db.execute("INSERT INTO ma_conversation_task_scopes VALUES(?1,'p','[]')",[&task.id])?;
        }
        assert!(view().is_err());
        store.connection.lock().unwrap().execute("UPDATE ma_conversation_plans SET applied_request_id='managed-p' WHERE id='p'",[])?;
        // Older public runtime events must never authorize a lookup in the user's home.
        store.append_runtime_event(AppendRuntimeEventInput{run_id:run.id.clone(),event_type:"managed_turn".into(),
            payload:json!({"phase":"completed","identity":{"threadId":"other-private-session"}})})?;
        store.connection.lock().unwrap().execute("UPDATE ma_runs SET session_locator_json=json_set(session_locator_json,'$.managedTurn','completed','$.threadId','other-private-session') WHERE id=?1",[&run.id])?;
        assert!(view().is_err());
        store.connection.lock().unwrap().execute("UPDATE ma_runs SET session_locator_json=json_remove(session_locator_json,'$.managedTurn','$.threadId') WHERE id=?1",[&run.id])?;
        for phase in ["dispatched","accepted","completed"] {
            store.checkpoint_managed_turn(&run.id,phase,json!({"threadId":"owned-thread"}),AgentRuntimeKind::Codex,cwd)?;
        }
        assert!(matches!(view()?, ConversationRunHistory::Managed(_)));
        assert!(store.conversation_run_locator("another-project",&run.id).is_err());
        store.connection.lock().unwrap().execute("UPDATE ma_runs SET session_locator_json=json_set(session_locator_json,'$.threadId','other-private-session') WHERE id=?1",[&run.id])?;
        assert!(view().is_err());
        Ok(())
    }

    #[test]
    fn brand_migration_updates_generated_references_and_contracts_without_rewriting_user_text() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "brand migration");
        let historical = String::from_utf8(vec![99, 99, 45, 100, 111, 99, 107]).unwrap();
        let old_variable = historical.to_ascii_uppercase().replace('-', "_");
        let old_contract = MESSAGE_TASK_ACCEPTANCE.replace("CREWVELA", &old_variable);
        let body = format!("User text must retain {historical}:// and {old_variable}_CLI exactly");
        assert!(fixture.store().enqueue_cli_message_task(&project.id, &agent.id, "managed-brand", &body).unwrap());
        assert!(fixture.store().enqueue_cli_message_task(&project.id, &agent.id, "custom-brand", &body).unwrap());
        let business = fixture.task(&project.id, &agent.id, "user task");
        fixture.store().rebuild_knowledge_index(&project.id).unwrap();
        let connection = fixture.store().connection.lock().unwrap();
        connection.execute("DELETE FROM ma_meta WHERE key='crewvela_namespace_v1'", []).unwrap();
        connection.execute("UPDATE ma_tasks SET acceptance_criteria=?1 WHERE id=?2",
            params![old_contract, "cli-message-managed-brand"]).unwrap();
        let custom_contract = format!("{old_contract}\nUser-added constraint");
        connection.execute("UPDATE ma_tasks SET acceptance_criteria=?1 WHERE id=?2",
            params![custom_contract, "cli-message-custom-brand"]).unwrap();
        connection.execute("UPDATE ma_tasks SET description=?1, acceptance_criteria=?2 WHERE id=?3",
            params![body, old_contract, business.id]).unwrap();
        connection.execute("UPDATE ma_knowledge_documents SET source_uri=replace(source_uri,'crewvela://',?1)",
            params![format!("{historical}://")]).unwrap();
        let custom_uri = format!("{historical}://user-supplied/reference");
        connection.execute("UPDATE ma_knowledge_documents SET source_uri=?1 WHERE source_type='task' AND source_id=?2",
            params![custom_uri, business.id]).unwrap();
        let before_content: String = connection.query_row(
            "SELECT content FROM ma_knowledge_documents WHERE source_type='task' AND source_id=?1", [&business.id], |row| row.get(0),
        ).unwrap();
        migrate_brand_references(&connection).unwrap();
        migrate_brand_references(&connection).unwrap();
        let project_uri: String = connection.query_row("SELECT source_uri FROM ma_knowledge_documents WHERE source_type='project' AND source_id=?1",
            [&project.id], |row| row.get(0)).unwrap();
        assert_eq!(project_uri, format!("crewvela://projects/{}", project.id));
        let (description, criteria): (String, String) = connection.query_row(
            "SELECT description,acceptance_criteria FROM ma_tasks WHERE id='cli-message-managed-brand'", [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(description, body);
        assert_eq!(criteria, MESSAGE_TASK_ACCEPTANCE);
        let custom: String = connection.query_row("SELECT acceptance_criteria FROM ma_tasks WHERE id='cli-message-custom-brand'", [], |row| row.get(0)).unwrap();
        assert_eq!(custom, custom_contract);
        let business_criteria: String = connection.query_row("SELECT acceptance_criteria FROM ma_tasks WHERE id=?1", [&business.id], |row| row.get(0)).unwrap();
        assert_eq!(business_criteria, old_contract);
        let (uri, content): (String, String) = connection.query_row("SELECT source_uri,content FROM ma_knowledge_documents WHERE source_type='task' AND source_id=?1",
            [&business.id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(uri, custom_uri);
        assert_eq!(content, before_content);
    }

    #[test]
    fn brand_reference_migration_rolls_back_failures_and_retries_on_reopen() {
        let fixture = TestStore::new();
        let project = fixture.project();
        fixture.store().rebuild_knowledge_index(&project.id).unwrap();
        let historical = String::from_utf8(vec![99, 99, 45, 100, 111, 99, 107]).unwrap();
        let previous_uri = format!("{historical}://projects/{}", project.id);
        {
            let connection = fixture.store().connection.lock().unwrap();
            connection.execute("DELETE FROM ma_meta WHERE key='crewvela_namespace_v1'", []).unwrap();
            connection.execute("UPDATE ma_knowledge_documents SET source_uri=?1 WHERE source_type='project'", [&previous_uri]).unwrap();
            connection.execute_batch("CREATE TRIGGER deny_brand_update BEFORE UPDATE OF source_uri ON ma_knowledge_documents
                BEGIN SELECT RAISE(ABORT,'fixture migration failure'); END;").unwrap();
            assert!(migrate_brand_references(&connection).is_err());
            let uri: String = connection.query_row("SELECT source_uri FROM ma_knowledge_documents WHERE source_type='project'", [], |row| row.get(0)).unwrap();
            assert_eq!(uri, previous_uri);
            let completed: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM ma_meta WHERE key='crewvela_namespace_v1')", [], |row| row.get(0)).unwrap();
            assert!(!completed);
            connection.execute_batch("DROP TRIGGER deny_brand_update;").unwrap();
        }
        let reopened = MultiAgentStore::open_at(fixture.root.join("control.db")).unwrap();
        let connection = reopened.connection.lock().unwrap();
        let uri: String = connection.query_row("SELECT source_uri FROM ma_knowledge_documents WHERE source_type='project'", [], |row| row.get(0)).unwrap();
        assert_eq!(uri, format!("crewvela://projects/{}", project.id));
    }

    #[test]
    fn agent_avatar_persists_validates_and_survives_runtime_switch() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "avatar member");
        assert_eq!(agent.avatar_id, None);
        let mut input = agent_edit(&agent);
        input.avatar_id = Some("orbit".into());
        let selected = fixture.store().update_agent(input).unwrap();
        assert_eq!(selected.avatar_id.as_deref(), Some("orbit"));
        let reopened = MultiAgentStore::open_at(fixture.root.join("control.db")).unwrap();
        assert_eq!(
            reopened
                .agent_profile(&agent.id)
                .unwrap()
                .avatar_id
                .as_deref(),
            Some("orbit")
        );
        let mut input = agent_edit(&selected);
        input.runtime_kind = AgentRuntimeKind::Pi;
        let switched = fixture.store().update_agent(input).unwrap();
        assert_eq!(switched.avatar_id.as_deref(), Some("orbit"));
        let mut invalid = agent_edit(&switched);
        invalid.avatar_id = Some("https://example.test/avatar.svg".into());
        assert!(fixture
            .store()
            .update_agent(invalid)
            .unwrap_err()
            .to_string()
            .contains("未知成员头像"));
        assert_eq!(
            fixture
                .store()
                .agent_profile(&agent.id)
                .unwrap()
                .avatar_id
                .as_deref(),
            Some("orbit")
        );
        let input: CreateAgentInput = serde_json::from_value(serde_json::json!({"projectId":project.id,"name":"new","runtimeKind":"pi","role":"worker","avatarId":"unknown"})).unwrap();
        assert!(fixture
            .store()
            .create_agent(input)
            .unwrap_err()
            .to_string()
            .contains("未知成员头像"));
        let mut reset = agent_edit(&switched);
        reset.avatar_id = None;
        assert_eq!(fixture.store().update_agent(reset).unwrap().avatar_id, None);
    }

    #[test]
    fn imported_agent_avatar_survives_source_removal_and_database_reopen() -> Result<()> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let fixture = TestStore::new();
        let store = fixture.store();
        let project = fixture.project();
        let source = fixture.root.join("portrait.png");
        image::DynamicImage::new_rgb8(320, 180).save(&source)?;
        let avatar = store.import_agent_avatar(&source)?;
        assert_eq!(store.import_agent_avatar(&source)?, avatar);
        let agent = store.create_agent(serde_json::from_value(serde_json::json!({
            "projectId": project.id, "name": "Custom avatar", "runtimeKind": "codex", "role": "worker", "avatarId": avatar,
        }))?)?;
        std::fs::remove_file(&source)?;

        let reopened = MultiAgentStore::open_at(fixture.root.join("control.db"))?;
        assert_eq!(reopened.agent_profile(&agent.id)?.avatar_id.as_deref(), Some(avatar.as_str()));
        let source = reopened.agent_avatar_image(&avatar)?;
        let png = STANDARD.decode(source.strip_prefix("data:image/png;base64,").unwrap())?;
        let image = image::load_from_memory_with_format(&png, image::ImageFormat::Png)?;
        assert_eq!((image.width(), image.height()), (192, 192));
        let invalid = fixture.root.join("invalid.png");
        std::fs::write(&invalid, b"<svg xmlns='http://www.w3.org/2000/svg'/>")?;
        assert!(reopened.import_agent_avatar(&invalid).is_err());
        assert!(reopened.agent_avatar_image("../../portrait.png").is_err());
        let mut edit = agent_edit(&agent);
        edit.avatar_id = Some(format!("imported:{}", "0".repeat(64)));
        assert!(reopened.update_agent(edit).is_err());
        assert_eq!(reopened.agent_profile(&agent.id)?.avatar_id.as_deref(), Some(avatar.as_str()));
        let mut reset = agent_edit(&agent);
        reset.avatar_id = None;
        assert!(reopened.update_agent(reset)?.avatar_id.is_none());
        assert_eq!(reopened.agent_avatar_image(&avatar)?, source);
        Ok(())
    }

    #[test]
    fn agent_persona_round_trips_and_custom_text_clears_on_builtin() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "persona member");
        // 默认人格是务实高效（efficient），且无自定义文本。
        assert_eq!(agent.persona, AgentPersona::Efficient);
        assert_eq!(agent.persona_custom, "");

        // custom + 自由文本：持久化并跨开库存活，超长文本截断到 120 字。
        let mut input = agent_edit(&agent);
        input.persona = AgentPersona::Custom;
        input.persona_custom = "  先复盘历史任务再动手，遇到歧义先提问。  ".to_string();
        let custom = fixture.store().update_agent(input).unwrap();
        assert_eq!(custom.persona, AgentPersona::Custom);
        assert_eq!(
            custom.persona_custom,
            "先复盘历史任务再动手，遇到歧义先提问。"
        );
        let reopened = MultiAgentStore::open_at(fixture.root.join("control.db")).unwrap();
        assert_eq!(reopened.agent_profile(&agent.id).unwrap().persona, AgentPersona::Custom);

        // 切回内置人格：残留的自由文本被清空，不再进入派单提示词。
        let mut builtin = agent_edit(&custom);
        builtin.persona = AgentPersona::Rigorous;
        let cleared = fixture.store().update_agent(builtin).unwrap();
        assert_eq!(cleared.persona, AgentPersona::Rigorous);
        assert_eq!(cleared.persona_custom, "");

        // 新建成员也可直接带人格；缺省字段回落 efficient（旧客户端兼容路径）。
        let created = fixture
            .store()
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: project.id.clone(),
                name: "persona create".to_string(),
                runtime_kind: AgentRuntimeKind::Codex,
                role: AgentRole::Reviewer,
                description: String::new(),
                model: None,
                permission_profile: PermissionProfile::Auto,
                max_concurrency: 1,
                persona: AgentPersona::Innovative,
                persona_custom: String::new(),
            })
            .unwrap();
        assert_eq!(created.persona, AgentPersona::Innovative);
        let legacy: CreateAgentInput = serde_json::from_value(serde_json::json!({
            "projectId": project.id,
            "name": "legacy persona",
            "runtimeKind": "codex",
            "role": "worker"
        }))
        .unwrap();
        let legacy_created = fixture.store().create_agent(legacy).unwrap();
        assert_eq!(legacy_created.persona, AgentPersona::Efficient);
    }

    fn agent_edit(agent: &AgentProfile) -> UpdateAgentInput {
        UpdateAgentInput {
            avatar_id: agent.avatar_id.clone(),
            project_id: agent.project_id.clone(),
            agent_id: agent.id.clone(),
            name: agent.name.clone(),
            runtime_kind: agent.runtime_kind,
            role: agent.role,
            description: agent.description.clone(),
            persona: agent.persona,
            persona_custom: agent.persona_custom.clone(),
            model: agent.model.clone(),
            enabled: agent.enabled,
        }
    }

    #[test]
    fn zcode_cli_agent_survives_creation_reopen_and_edit() {
        let mut fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.store().create_agent(CreateAgentInput {
            avatar_id: None, project_id: project.id.clone(), name: "ZCode CLI".into(),
            runtime_kind: AgentRuntimeKind::ZCodeCli, role: AgentRole::Worker,
            description: String::new(), model: None, permission_profile: PermissionProfile::Auto,
            max_concurrency: 1, persona: Default::default(), persona_custom: String::new(),
        }).unwrap();
        assert_eq!(fixture.store().snapshot(&project.id).unwrap().agents[0].runtime_kind, AgentRuntimeKind::ZCodeCli);
        fixture.store.take();
        fixture.store = Some(MultiAgentStore::open_at(fixture.root.join("control.db")).unwrap());
        let mut edit = agent_edit(&agent);
        edit.name = "已重命名".into();
        fixture.store().update_agent(edit).unwrap();
        assert_eq!(fixture.store().snapshot(&project.id).unwrap().agents[0].name, "已重命名");
    }

    #[test]
    fn collaboration_cli_admission_rejects_new_chat_and_preserves_legacy_desktop() {
        let fixture = TestStore::new();
        let project = fixture.project();
        for runtime in super::super::COLLABORATION_CLI_RUNTIMES {
            let input:CreateAgentInput=serde_json::from_value(serde_json::json!({"projectId":project.id,"name":runtime.as_str(),"runtimeKind":runtime,"role":"worker"})).unwrap();
            fixture.store().create_agent(input).unwrap();
        }
        assert_eq!(fixture.store().snapshot(&project.id).unwrap().agents.len(),7);
        for runtime in [AgentRuntimeKind::ZCode,AgentRuntimeKind::ClaudeChat,AgentRuntimeKind::CodexChat,AgentRuntimeKind::KimiChat,AgentRuntimeKind::PiChat,AgentRuntimeKind::OpenCodeChat] {
            let input:CreateAgentInput=serde_json::from_value(serde_json::json!({"projectId":project.id,"name":"rejected","runtimeKind":runtime,"role":"worker"})).unwrap();
            assert!(fixture.store().create_agent(input).unwrap_err().to_string().contains("CLI"));
        }
        assert_eq!(fixture.store().snapshot(&project.id).unwrap().agents.len(),7,"rejected creation is side-effect free");
        let cli=fixture.store().snapshot(&project.id).unwrap().agents[0].clone();
        for target in [AgentRuntimeKind::ZCode,AgentRuntimeKind::ClaudeChat] {
            let mut edit=agent_edit(&cli);edit.runtime_kind=target;
            assert!(fixture.store().update_agent(edit).is_err());
            assert_eq!(fixture.store().agent_profile(&cli.id).unwrap().runtime_kind,cli.runtime_kind);
        }
        // Simulate a previously persisted Desktop member, rather than bypassing the new API.
        fixture.store().connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind='zcode',model='desktop/original-model' WHERE id=?1",[&cli.id]).unwrap();
        let old=fixture.store().agent_profile(&cli.id).unwrap();
        let task:CreateTaskInput=serde_json::from_value(serde_json::json!({"projectId":project.id,"title":"original task","description":"original scope","assigneeAgentId":old.id,"workspaceMode":"shared_write"})).unwrap();
        let task=fixture.store().create_task(task).unwrap();
        let reopened=MultiAgentStore::open_at(fixture.root.join("control.db")).unwrap();
        let old=reopened.agent_profile(&old.id).unwrap();assert!(old.enabled);assert_eq!(old.runtime_kind,AgentRuntimeKind::ZCode);assert_eq!(old.model.as_deref(),Some("desktop/original-model"));
        let mut edit=agent_edit(&old);edit.description="updated role".into();
        let updated=reopened.update_agent(edit).unwrap();
        assert_eq!(updated.runtime_kind,AgentRuntimeKind::ZCode);assert_eq!(updated.model,old.model);assert_eq!(updated.permission_profile,old.permission_profile);
        assert_eq!(reopened.snapshot(&project.id).unwrap().tasks.iter().find(|item|item.id==task.id).unwrap().assignee_agent_id.as_deref(),Some(old.id.as_str()));
        assert!(reopened.conversation_snapshot(&project.id).unwrap().supported_discussion_runtimes.contains(&AgentRuntimeKind::ZCode));
        let claimed=reopened.claim_task(ClaimTaskInput{task_id:task.id,agent_id:old.id.clone(),lease_seconds:120}).unwrap();
        assert_eq!(claimed.agent_id,old.id,"legacy queued work still uses the original member");
    }

    #[test]
    fn durable_message_retry_is_idempotent_and_rejects_conflicting_payload() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let input = SendCollaborationMessageInput {
            project_id: project.id.clone(), task_id: None, sender_type: "user".into(),
            sender_id: None, target_agent_id: None, body: "durable payload".into(),
        };
        let first = fixture.store().send_collaboration_message_with_id(input.clone(), "outbox-stable").unwrap();
        let retry = fixture.store().send_collaboration_message_with_id(input.clone(), "outbox-stable").unwrap();
        assert_eq!(first.created_at, retry.created_at);
        assert_eq!(first.id, retry.id);
        let mut conflict = input;
        conflict.body = "different payload".into();
        assert!(fixture.store().send_collaboration_message_with_id(conflict, "outbox-stable").is_err());
        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(snapshot.messages.len(), 1);
        assert_eq!(snapshot.events.iter().filter(|event| event.event_type == "message.sent").count(), 1);
    }

    #[test]
    fn legacy_knowledge_documents_are_indexed_before_update_triggers_run() {
        for missing_table in [false, true] {
            let fixture = TestStore::new();
            let project = fixture.project();
            fixture.store().record_message(&project.id, "user", "historical knowledge marker").unwrap();
            fixture.store().rebuild_knowledge_index(&project.id).unwrap();
            {
                let connection = fixture.store().connection.lock().unwrap();
                connection.execute("DELETE FROM ma_meta WHERE key = 'knowledge_fts_backfilled_v1'", []).unwrap();
                if missing_table {
                    connection.execute_batch("DROP TRIGGER ma_knowledge_documents_ai; DROP TRIGGER ma_knowledge_documents_ad; DROP TRIGGER ma_knowledge_documents_au; DROP TABLE ma_knowledge_fts;").unwrap();
                } else {
                    connection.execute("INSERT INTO ma_knowledge_fts(ma_knowledge_fts) VALUES('delete-all')", []).unwrap();
                    let count: i64 = connection.query_row("SELECT COUNT(*) FROM ma_knowledge_fts", [], |row| row.get(0)).unwrap();
                    assert!(count > 0, "external content count cannot detect the empty index");
                }
                migrate_knowledge_fts(&connection).unwrap();
                migrate_knowledge_fts(&connection).unwrap();
                let matches: i64 = connection.query_row("SELECT COUNT(*) FROM ma_knowledge_fts WHERE ma_knowledge_fts MATCH 'historical'", [], |row| row.get(0)).unwrap();
                assert_eq!(matches, 1);
            }
            let results = fixture.store().search_knowledge(SearchKnowledgeInput {
                project_id: project.id, query: "historical".into(), source_types: vec![], task_id: None, limit: 20, offset: 0,
            }).unwrap();
            assert_eq!(results.len(), 1);
        }
    }

    #[test]
    fn runtime_switch_lifecycle_prevents_claiming_until_new_binding_is_ready() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "成员");
        fixture
            .store()
            .enqueue_cli_message_task(&project.id, &agent.id, "serialized", "消息")
            .unwrap();
        let store = fixture.store().clone();
        let agent_id = agent.id.clone();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let guard = fixture.store().agent_lifecycle_guard();
        let handle = std::thread::spawn(move || {
            started_tx.send(()).unwrap();
            let run = store
                .claim_task(ClaimTaskInput {
                    task_id: "cli-message-serialized".into(),
                    agent_id: agent_id.clone(),
                    lease_seconds: 60,
                })
                .unwrap();
            done_tx
                .send((run, store.agent_profile(&agent_id).unwrap().runtime_kind))
                .unwrap();
        });
        started_rx.recv().unwrap();
        assert!(done_rx
            .recv_timeout(std::time::Duration::from_millis(50))
            .is_err());
        let mut edit = agent_edit(&agent);
        edit.runtime_kind = AgentRuntimeKind::Pi;
        fixture.store().update_agent(edit).unwrap();
        fixture
            .store()
            .link_agent_app_session(&project.id, &agent.id, "new-pi-session")
            .unwrap();
        drop(guard);
        let (_, kind) = done_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(kind, AgentRuntimeKind::Pi);
        handle.join().unwrap();
        assert_eq!(
            fixture
                .store()
                .snapshot(&project.id)
                .unwrap()
                .agent_sessions[0]
                .app_session_id
                .as_deref(),
            Some("new-pi-session")
        );
    }

    #[test]
    fn editable_agent_keeps_task_identity_and_disabled_members_do_not_claim() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "原成员");
        let task = fixture.task(&project.id, &agent.id, "原任务");
        let second = fixture.agent(&project.id, "同 Runtime 第二成员");
        let mut edit = agent_edit(&agent);
        edit.name = "后端开发".into();
        edit.role = AgentRole::Reviewer;
        edit.description = "负责数据接口".into();
        edit.model = Some("custom-model".into());
        edit.enabled = false;
        let saved = fixture.store().update_agent(edit).unwrap();
        assert_eq!(saved.id, agent.id);
        assert_eq!(saved.created_at, agent.created_at);
        assert_eq!(saved.name, "后端开发");
        assert_eq!(saved.model.as_deref(), Some("custom-model"));
        assert_eq!(saved.role, AgentRole::Reviewer);
        assert!(!saved.enabled);
        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        let original = snapshot
            .tasks
            .iter()
            .find(|item| item.id == task.id)
            .unwrap();
        assert_eq!(original.assignee_agent_id, Some(agent.id.clone()));
        assert_eq!(original.status, task.status);
        assert_eq!(snapshot.agents.len(), 2);
        assert_eq!(second.runtime_kind, saved.runtime_kind);
        assert!(fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id,
                agent_id: agent.id,
                lease_seconds: 60
            })
            .is_err());
    }

    #[test]
    fn removing_member_checks_current_tasks_and_preserves_history() {
        let fixture = TestStore::new();
        let store = fixture.store();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "待移除");
        let other = fixture.agent(&project.id, "接收成员");
        let task = fixture.task(&project.id, &agent.id, "未完成开发");
        let review = fixture.task(&project.id, &other.id, "未完成审查");
        store.connection.lock().unwrap().execute(
            "UPDATE ma_tasks SET reviewer_agent_id=?1 WHERE id=?2", params![agent.id, review.id],
        ).unwrap();
        let blockers = store.remove_agent(&project.id, &agent.id, true).unwrap();
        assert!(blockers.contains(&task.title) && blockers.contains(&review.title));
        assert!(store.agent_profile(&agent.id).unwrap().enabled);
        store.reassign_task(&task.id, &other.id).unwrap();
        store.connection.lock().unwrap().execute("UPDATE ma_tasks SET status='done' WHERE id=?1", [&review.id]).unwrap();
        assert!(store.remove_agent(&project.id, &agent.id, false).unwrap().is_empty());
        let before = store.agent_profile(&agent.id).unwrap();
        assert!(before.enabled && before.removed_at.is_none());
        assert!(store.remove_agent(&project.id, &agent.id, true).unwrap().is_empty());
        let removed = store.agent_profile(&agent.id).unwrap();
        assert!(!removed.enabled && removed.removed_at.is_some());
        assert_eq!(removed.name, before.name);
        assert!(store.update_agent(agent_edit(&before)).is_err());
        assert!(store.remove_agent(&project.id, &agent.id, true).unwrap().is_empty());
        let snapshot = MultiAgentStore::open_at(fixture.root.join("control.db")).unwrap().snapshot(&project.id).unwrap();
        assert_eq!(snapshot.tasks.iter().find(|task| task.id == review.id).unwrap().reviewer_agent_id.as_deref(), Some(agent.id.as_str()));
        assert_eq!(snapshot.agents.iter().find(|row| row.id == agent.id).unwrap().removed_at, removed.removed_at);
        assert_eq!(snapshot.events.iter().filter(|event| event.event_type == "agent.removed").count(), 1);
    }

    #[test]
    fn editable_agent_rejects_active_execution_but_switches_idle_historical_runtime() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "活动成员");
        let task = fixture.task(&project.id, &agent.id, "执行任务");
        fixture
            .store()
            .enqueue_cli_message_task(&project.id, &agent.id, "active-test", "执行")
            .unwrap();
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: "cli-message-active-test".into(),
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        let mut edit = agent_edit(&agent);
        edit.name = "不应生效".into();
        assert!(fixture
            .store()
            .update_agent(edit)
            .unwrap_err()
            .to_string()
            .contains("正在执行"));
        assert_eq!(
            fixture.store().snapshot(&project.id).unwrap().agents[0].name,
            agent.name
        );
        let connection = fixture.store().connection.lock().unwrap();
        connection
            .execute(
                "UPDATE ma_runs SET status='succeeded' WHERE id=?1",
                [&run.id],
            )
            .unwrap();
        drop(connection);
        let mut edit = agent_edit(&agent);
        edit.runtime_kind = AgentRuntimeKind::Pi;
        assert_eq!(
            fixture.store().update_agent(edit).unwrap().runtime_kind,
            AgentRuntimeKind::Pi
        );
        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(
            snapshot
                .runs
                .iter()
                .find(|item| item.id == run.id)
                .unwrap()
                .status,
            RunStatus::Succeeded
        );
        let unbound = fixture.agent(&project.id, "未运行成员");
        let mut edit = agent_edit(&unbound);
        edit.runtime_kind = AgentRuntimeKind::Pi;
        assert_eq!(
            fixture.store().update_agent(edit).unwrap().runtime_kind,
            AgentRuntimeKind::Pi
        );
        fixture
            .store()
            .link_agent_app_session(&project.id, &unbound.id, "session")
            .unwrap();
        let mut edit = agent_edit(&unbound);
        edit.runtime_kind = AgentRuntimeKind::OpenCode;
        assert_eq!(
            fixture.store().update_agent(edit).unwrap().runtime_kind,
            AgentRuntimeKind::OpenCode
        );
        assert!(fixture
            .store()
            .snapshot(&project.id)
            .unwrap()
            .agent_sessions
            .iter()
            .find(|item| item.agent_id == unbound.id)
            .unwrap()
            .app_session_id
            .is_none());
        assert_eq!(
            fixture
                .store()
                .snapshot(&project.id)
                .unwrap()
                .tasks
                .iter()
                .find(|t| t.id == task.id)
                .unwrap()
                .assignee_agent_id
                .as_deref(),
            Some(agent.id.as_str())
        );
    }

    #[test]
    fn editable_agent_rejects_removed_runtime_and_cross_project_edits() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "成员");
        let mut edit = agent_edit(&agent);
        edit.runtime_kind = AgentRuntimeKind::OhMyPi;
        assert!(fixture.store().update_agent(edit).is_err());
        let mut edit = agent_edit(&agent);
        edit.runtime_kind = AgentRuntimeKind::OhMyPi;
        edit.enabled = false;
        assert!(fixture.store().update_agent(edit).is_err());
        let mut edit = agent_edit(&agent);
        edit.project_id = fixture.project().id;
        assert!(fixture.store().update_agent(edit).is_err());
        fixture
            .store()
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE ma_agents SET runtime_kind='oh_my_pi', enabled=0 WHERE id=?1",
                [&agent.id],
            )
            .unwrap();
        let mut edit = agent_edit(&agent);
        edit.runtime_kind = AgentRuntimeKind::OhMyPi;
        edit.enabled = false;
        assert!(!fixture.store().update_agent(edit).unwrap().enabled);
        let mut edit = agent_edit(&agent);
        edit.runtime_kind = AgentRuntimeKind::OhMyPi;
        edit.enabled = true;
        assert!(fixture.store().update_agent(edit).is_err());
    }

    #[test]
    fn directed_mail_is_readable_by_the_recipient_across_tasks_only() {
        let db = TestStore::new();
        let p = db.project();
        let a = db.agent(&p.id, "writer");
        let b = db.agent(&p.id, "reader");
        let c = db.agent(&p.id, "observer");
        let source = db.task(&p.id, &a.id, "source");
        let target = db.task(&p.id, &b.id, "target");
        let other = db.task(&p.id, &c.id, "other");
        for (address, body) in [(Some(b.id.clone()), "for-reader"), (None, "source-task-only")] {
            db.store().send_collaboration_message(SendCollaborationMessageInput {
                project_id: p.id.clone(), sender_type: "agent".into(), sender_id: Some(a.id.clone()),
                task_id: Some(source.id.clone()), target_agent_id: address, body: body.into(),
            }).unwrap();
        }
        let received = db.store().scoped_messages(&p.id, &target.id, &b.id, 20).unwrap();
        assert!(received.iter().any(|m| m.body == "for-reader"));
        assert!(!received.iter().any(|m| m.body == "source-task-only"));
        let unrelated = db.store().scoped_messages(&p.id, &other.id, &c.id, 20).unwrap();
        assert!(!unrelated.iter().any(|m| m.body == "for-reader" || m.body == "source-task-only"));
    }

    #[test]
    fn claim_rechecks_project_and_retry_deadline_before_creating_a_run() {
        let db = TestStore::new();
        let p = db.project();
        let a = db.agent(&p.id, "worker");
        let t = db.task(&p.id, &a.id, "retry");
        let claim = || db.store().claim_task(ClaimTaskInput { agent_id: a.id.clone(), task_id: t.id.clone(), lease_seconds: 60 });
        for state in [ProjectStatus::Paused, ProjectStatus::Cancelled, ProjectStatus::Completed] {
            db.store().set_project_status(&p.id, state).unwrap();
            assert!(claim().is_err());
        }
        db.store().set_project_status(&p.id, ProjectStatus::Active).unwrap();
        db.store().connection.lock().unwrap().execute("UPDATE ma_tasks SET next_retry_at=?1 WHERE id=?2", params![now_millis()+60_000, t.id]).unwrap();
        assert!(claim().is_err());
        assert!(db.store().snapshot(&p.id).unwrap().runs.is_empty());
        db.store().connection.lock().unwrap().execute("UPDATE ma_tasks SET next_retry_at=0 WHERE id=?1", [&t.id]).unwrap();
        assert_eq!(claim().unwrap().attempt, 1);
    }

    #[test]
    fn an_approval_is_idempotent_and_cannot_outlive_its_run() {
        let db = TestStore::new();
        let p = db.project();
        let a = db.agent(&p.id, "worker");
        let t = db.task(&p.id, &a.id, "approval");
        let run = db.store().claim_task(ClaimTaskInput { agent_id:a.id,task_id:t.id.clone(),lease_seconds:60 }).unwrap();
        let request = RequestApprovalInput { project_id:p.id.clone(),task_id:Some(t.id),run_id:Some(run.id.clone()),approval_type:"manual".into(),summary:"confirm".into() };
        let first = db.store().request_approval(request.clone()).unwrap();
        assert_eq!(db.store().request_approval(request.clone()).unwrap().id, first.id);
        db.store().fail_run_permanently(&run.id, "stopped", None).unwrap();
        assert!(db.store().request_approval(request).is_err());
        let approvals = db.store().snapshot(&p.id).unwrap().approvals;
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals[0].status, "cancelled");
        db.store().connection.lock().unwrap().execute(
            "UPDATE ma_approvals SET status='pending', decided_at=NULL WHERE id=?1", [&first.id],
        ).unwrap();
        let reopened = MultiAgentStore::open_at(db.root.join("control.db")).unwrap();
        assert_eq!(reopened.snapshot(&p.id).unwrap().approvals[0].status, "cancelled");
    }

    #[test]
    fn retired_cli_members_survive_database_reopen_but_cannot_claim() {
        let db = TestStore::new();
        let p = db.project();
        let a = db.agent(&p.id, "legacy");
        let t = db.task(&p.id, &a.id, "history");
        db.store().connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind='minimax_code' WHERE id=?1",[&a.id]).unwrap();
        let reopened = MultiAgentStore::open_at(db.root.join("control.db")).unwrap();
        let old = reopened.agent_profile(&a.id).unwrap();
        assert!(old.runtime_kind.is_retired());
        assert!(!old.enabled);
        assert!(reopened.claim_task(ClaimTaskInput {agent_id:a.id,task_id:t.id,lease_seconds:60}).is_err());
    }

    #[test]
    fn unknown_runtime_members_remain_readable_but_disabled_after_reopen() {
        let db = TestStore::new();
        let p = db.project();
        let a = db.agent(&p.id, "legacy");
        let t = db.task(&p.id, &a.id, "history");
        db.store().connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind='removed_runtime' WHERE id=?1", [&a.id]).unwrap();
        let reopened = MultiAgentStore::open_at(db.root.join("control.db")).unwrap();
        let snapshot = reopened.snapshot(&p.id).unwrap();
        let historical = snapshot.agents.iter().find(|agent| agent.id == a.id).unwrap();
        assert_eq!(historical.runtime_kind, AgentRuntimeKind::Unsupported);
        assert!(!historical.enabled);
        assert!(snapshot.tasks.iter().any(|task| task.id == t.id));
        assert!(reopened.runnable_tasks(&p.id).unwrap().is_empty());
        assert!(reopened.claim_task(ClaimTaskInput { agent_id: a.id, task_id: t.id, lease_seconds: 60 }).is_err());
    }

    #[test]
    fn cli_message_ids_reject_workspace_path_components() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "receiver");
        for id in ["../escape", "nested/path", "nested\\path", "C:escape", "..", "with.dot", "with space", "\u{0}"] {
            assert!(fixture.store().enqueue_cli_message_task(&project.id, &agent.id, id, "message").is_err(), "accepted unsafe message ID {id:?}");
        }
        assert!(fixture.store().snapshot(&project.id).unwrap().tasks.is_empty());
        assert!(fixture.store().enqueue_cli_message_task(&project.id, &agent.id, "safe-message_123", "message").unwrap());
    }

    #[test]
    fn interrupted_cli_message_turns_never_replay_after_pause_or_stop_confirmation() {
        for unverified_stop in [false, true] {
            let fixture = TestStore::new();
            let store = fixture.store();
            let project = fixture.project();
            let agent = fixture.agent(&project.id, "receiver");
            let peer = fixture.agent(&project.id, "original sender");
            let message_id = "already-replied";
            let body = "one isolated reply";
            assert!(store.enqueue_cli_message_task(&project.id, &agent.id, message_id, body).unwrap());
            let task_id = format!("cli-message-{message_id}");
            let run = store.claim_task(ClaimTaskInput {
                task_id: task_id.clone(), agent_id: agent.id.clone(), lease_seconds: 60,
            }).unwrap();
            store.send_collaboration_message(SendCollaborationMessageInput {
                project_id: project.id.clone(), task_id: Some(task_id.clone()),
                sender_type: "agent".into(), sender_id: Some(agent.id.clone()),
                target_agent_id: Some(peer.id), body: "reply already sent".into(),
            }).unwrap();
            if unverified_stop {
                store.mark_run_stop_unverified(&run.id, "stop timeout after reply").unwrap();
            } else {
                store.pause_run(&run.id).unwrap();
            }
            let reopened = MultiAgentStore::open_at(fixture.root.join("control.db")).unwrap();
            if unverified_stop {
                assert!(reopened.confirm_run_stopped(&run.id, false).is_err());
                reopened.confirm_run_stopped(&run.id, true).unwrap();
            }
            reopened.set_project_status(&project.id, ProjectStatus::Active).unwrap();
            reopened.recover_interrupted_runs_at_startup().unwrap();
            // Retrying ingestion acknowledges the same durable message without
            // creating a fresh task or restarting an uncertain model turn.
            assert!(reopened.enqueue_cli_message_task(&project.id, &agent.id, message_id, body).unwrap());
            assert!(reopened.runnable_tasks(&project.id).unwrap().is_empty());
            assert!(reopened.claim_task(ClaimTaskInput {
                task_id: task_id.clone(), agent_id: agent.id, lease_seconds: 60,
            }).is_err());
            let snapshot = reopened.snapshot(&project.id).unwrap();
            assert_eq!(snapshot.tasks.iter().find(|task| task.id == task_id).unwrap().status, TaskStatus::Failed);
            assert_eq!(snapshot.runs.len(), 1);
            assert_eq!(snapshot.messages.iter().filter(|message| message.body == "reply already sent").count(), 1);
            if unverified_stop {
                let event = snapshot.events.iter().find(|event| event.event_type == "run.stop_confirmed").unwrap();
                assert_eq!(event.payload["retryAllowed"], false);
            }
        }
    }

    #[test]
    fn cli_message_tasks_cover_all_runtimes_and_survive_reopen_without_duplicates() {
        let mut fixture = TestStore::new();
        let project = fixture.project();
        fixture
            .store()
            .set_project_status(&project.id, ProjectStatus::Active)
            .unwrap();
        for kind in [
            AgentRuntimeKind::ClaudeCode,
            AgentRuntimeKind::Codex,
            AgentRuntimeKind::KimiCode,
            AgentRuntimeKind::Pi,
            AgentRuntimeKind::OpenCode,
        ] {
            let agent = fixture
                .store()
                .create_agent(CreateAgentInput {
                    avatar_id: None,
                    project_id: project.id.clone(),
                    name: kind.as_str().into(),
                    runtime_kind: kind,
                    role: AgentRole::Worker,
                    description: String::new(),
                    model: None,
                    permission_profile: PermissionProfile::Default,
                    max_concurrency: 1,
                persona: Default::default(),
                persona_custom: String::new(),
                })
                .unwrap();
            let original = fixture.task(&project.id, &agent.id, "原任务");
            let message_id = uuid::Uuid::new_v4().to_string();
            let store = fixture.store();
            assert!(store
                .enqueue_cli_message_task(&project.id, &agent.id, &message_id, "来自其他会话的消息")
                .unwrap());
            let database = fixture.root.join("control.db");
            fixture.store.take();
            fixture.store = Some(MultiAgentStore::open_at(database).unwrap());
            let store = fixture.store();
            assert!(store
                .enqueue_cli_message_task(&project.id, &agent.id, &message_id, "来自其他会话的消息")
                .unwrap());
            assert!(store
                .enqueue_cli_message_task(&project.id, &agent.id, &message_id, "其他内容")
                .is_err());
            let snapshot = store.snapshot(&project.id).unwrap();
            let id = format!("cli-message-{message_id}");
            assert_eq!(
                snapshot.tasks.iter().filter(|task| task.id == id).count(),
                1
            );
            let message_task = snapshot.tasks.iter().find(|task| task.id == id).unwrap();
            assert_eq!(message_task.workspace_mode, WorkspaceMode::ReadOnly);
            assert_eq!(
                message_task.assignee_agent_id.as_deref(),
                Some(agent.id.as_str())
            );
            assert_eq!(message_task.max_attempts, 1);
            assert!(message_task.reviewer_agent_id.is_none());
            let after = snapshot
                .tasks
                .iter()
                .find(|task| task.id == original.id)
                .unwrap();
            assert_eq!(after.status, original.status);
            assert_eq!(after.description, original.description);
        }
    }

    #[test]
    fn cli_message_tasks_respect_pauses_and_keep_the_original_recipient() {
        let fixture = TestStore::new();
        let store = fixture.store();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "收件人");
        let _replacement = fixture.agent(&project.id, "其他成员");
        for status in [ProjectStatus::Paused, ProjectStatus::Cancelled] {
            store.set_project_status(&project.id, status).unwrap();
            assert!(!store
                .enqueue_cli_message_task(&project.id, &agent.id, "pending", "消息")
                .unwrap());
        }
        assert!(store.snapshot(&project.id).unwrap().tasks.is_empty());
        store
            .set_project_status(&project.id, ProjectStatus::Draft)
            .unwrap();
        assert!(store
            .enqueue_cli_message_task(&project.id, &agent.id, "draft-message", "计划阶段的消息")
            .unwrap());
        assert_eq!(
            store.project(&project.id).unwrap().status,
            ProjectStatus::Draft
        );
        store
            .set_project_status(&project.id, ProjectStatus::Completed)
            .unwrap();
        assert!(store
            .enqueue_cli_message_task(&project.id, &agent.id, "completed-message", "完工后的消息")
            .unwrap());
        assert_eq!(
            store.project(&project.id).unwrap().status,
            ProjectStatus::Completed
        );
        store
            .set_project_status(&project.id, ProjectStatus::Active)
            .unwrap();
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE ma_agents SET enabled = 0 WHERE id = ?1",
                [&agent.id],
            )
            .unwrap();
        assert!(!store
            .enqueue_cli_message_task(&project.id, &agent.id, "pending", "消息")
            .unwrap());
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE ma_agents SET enabled = 1 WHERE id = ?1",
                [&agent.id],
            )
            .unwrap();
        store
            .set_agent_control(&project.id, &agent.id, AgentControlMode::Manual, None)
            .unwrap();
        assert!(!store
            .enqueue_cli_message_task(&project.id, &agent.id, "pending", "消息")
            .unwrap());
        store
            .set_agent_control(&project.id, &agent.id, AgentControlMode::Auto, None)
            .unwrap();
        assert!(store
            .enqueue_cli_message_task(&project.id, &agent.id, "pending", "消息")
            .unwrap());
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE ma_agents SET enabled = 0 WHERE id = ?1",
                [&agent.id],
            )
            .unwrap();
        store.route_runnable_tasks(&project.id).unwrap();
        let snapshot = store.snapshot(&project.id).unwrap();
        assert!(snapshot
            .tasks
            .iter()
            .all(|task| task.assignee_agent_id.as_deref() == Some(agent.id.as_str())));
        assert!(store.runnable_tasks(&project.id).unwrap().is_empty());
        let other_project = fixture.project();
        assert!(store
            .enqueue_cli_message_task(&other_project.id, &agent.id, "wrong-project", "消息")
            .is_err());
    }

    #[test]
    fn cli_message_task_obeys_the_receivers_active_run_limit() {
        let fixture = TestStore::new();
        let store = fixture.store();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "收件人");
        store
            .set_project_status(&project.id, ProjectStatus::Active)
            .unwrap();
        store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE ma_agents SET max_concurrency = 2 WHERE id = ?1",
                [&agent.id],
            )
            .unwrap();
        let original = fixture.task(&project.id, &agent.id, "正在执行的任务");
        let original_run = store
            .claim_task(ClaimTaskInput {
                task_id: original.id,
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        assert!(store
            .enqueue_cli_message_task(&project.id, &agent.id, "after-turn", "消息")
            .unwrap());
        assert!(store.runnable_tasks(&project.id).unwrap().is_empty());
        assert!(store
            .claim_task(ClaimTaskInput {
                task_id: "cli-message-after-turn".into(),
                agent_id: agent.id.clone(),
                lease_seconds: 60
            })
            .is_err());
        store
            .update_run(UpdateRunInput {
                run_id: original_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        store
            .finalize_successful_run(&original_run.id, "workspace", "已完成", true, None, None)
            .unwrap();
        store
            .claim_task(ClaimTaskInput {
                task_id: "cli-message-after-turn".into(),
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        let next = fixture.task(&project.id, &agent.id, "后续业务任务");
        assert!(store.runnable_tasks(&project.id).unwrap().is_empty());
        assert!(store
            .claim_task(ClaimTaskInput {
                task_id: next.id,
                agent_id: agent.id,
                lease_seconds: 60
            })
            .is_err());
    }

    #[test]
    fn cli_message_turns_finish_without_business_reviews_and_do_not_replay_after_restart() {
        let fixture = TestStore::new();
        let store = fixture.store();
        let project = fixture.project();
        store
            .set_project_status(&project.id, ProjectStatus::Active)
            .unwrap();
        for role in [AgentRole::Reviewer, AgentRole::Integrator] {
            let agent = store
                .create_agent(CreateAgentInput {
                    avatar_id: None,
                    project_id: project.id.clone(),
                    name: role.as_str().into(),
                    runtime_kind: AgentRuntimeKind::Codex,
                    role,
                    description: String::new(),
                    model: None,
                    permission_profile: PermissionProfile::Default,
                    max_concurrency: 1,
                persona: Default::default(),
                persona_custom: String::new(),
                })
                .unwrap();
            let message_id = uuid::Uuid::new_v4().to_string();
            store
                .enqueue_cli_message_task(&project.id, &agent.id, &message_id, "消息")
                .unwrap();
            let task_id = format!("cli-message-{message_id}");
            let run = store
                .claim_task(ClaimTaskInput {
                    task_id: task_id.clone(),
                    agent_id: agent.id.clone(),
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
                .finalize_successful_run(&run.id, "message", "已处理", false, None, None)
                .unwrap();
            let snapshot = store.snapshot(&project.id).unwrap();
            assert_eq!(
                snapshot
                    .tasks
                    .iter()
                    .find(|task| task.id == task_id)
                    .unwrap()
                    .status,
                TaskStatus::Done
            );
            assert!(snapshot.review_decisions.is_empty());

            let interrupted_id = uuid::Uuid::new_v4().to_string();
            store
                .enqueue_cli_message_task(&project.id, &agent.id, &interrupted_id, "中断的消息")
                .unwrap();
            let interrupted_task_id = format!("cli-message-{interrupted_id}");
            let interrupted = store
                .claim_task(ClaimTaskInput {
                    task_id: interrupted_task_id.clone(),
                    agent_id: agent.id,
                    lease_seconds: 60,
                })
                .unwrap();
            store
                .update_run(UpdateRunInput {
                    run_id: interrupted.id.clone(),
                    status: RunStatus::Running,
                    output_summary: None,
                    error: None,
                    session_locator: None,
                })
                .unwrap();
            store.recover_interrupted_runs().unwrap();
            let snapshot = store.snapshot(&project.id).unwrap();
            assert_eq!(
                snapshot
                    .tasks
                    .iter()
                    .find(|task| task.id == interrupted_task_id)
                    .unwrap()
                    .status,
                TaskStatus::Failed
            );
            assert!(snapshot
                .runs
                .iter()
                .find(|run| run.id == interrupted.id)
                .unwrap()
                .error
                .as_deref()
                .unwrap()
                .contains("未自动重试"));
            assert!(store.runnable_tasks(&project.id).unwrap().is_empty());
        }
    }

    #[test]
    fn legacy_runtime_is_disabled_without_reassigning_or_changing_task_status() {
        let mut fixture = TestStore::new();
        let project = fixture.project();
        let legacy = fixture.agent(&project.id, "旧成员");
        let replacement = fixture.agent(&project.id, "候选成员");
        let task = fixture.task(&project.id, &legacy.id, "待改派任务");
        let database = fixture.root.join("control.db");
        fixture.store.take();
        {
            let connection = Connection::open(&database).unwrap();
            connection
                .execute(
                    "UPDATE ma_agents SET runtime_kind = 'oh_my_pi' WHERE id = ?1",
                    [&legacy.id],
                )
                .unwrap();
        }
        fixture.store = Some(MultiAgentStore::open_at(database).unwrap());
        let store = fixture.store();
        let snapshot = store.snapshot(&project.id).unwrap();
        assert!(
            !snapshot
                .agents
                .iter()
                .find(|agent| agent.id == legacy.id)
                .unwrap()
                .enabled
        );
        assert_eq!(
            snapshot
                .tasks
                .iter()
                .find(|item| item.id == task.id)
                .unwrap()
                .status,
            task.status
        );
        store.route_runnable_tasks(&project.id).unwrap();
        let after = store.snapshot(&project.id).unwrap();
        assert_eq!(
            after
                .tasks
                .iter()
                .find(|item| item.id == task.id)
                .unwrap()
                .assignee_agent_id
                .as_deref(),
            Some(legacy.id.as_str())
        );
        assert_eq!(store.ensure_runnable_wakeups(&project.id).unwrap(), 0);
        assert!(store
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: legacy.id.clone(),
                lease_seconds: 60
            })
            .unwrap_err()
            .to_string()
            .contains("已禁用"));
        assert!(store
            .reassign_task(&task.id, &legacy.id)
            .unwrap_err()
            .to_string()
            .contains("已禁用"));
        let reassigned = store.reassign_task(&task.id, &replacement.id).unwrap();
        assert_eq!(
            reassigned.assignee_agent_id.as_deref(),
            Some(replacement.id.as_str())
        );
    }

    #[test]
    fn removed_runtime_cannot_be_created() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let error = fixture
            .store()
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: project.id,
                name: "旧成员".to_string(),
                runtime_kind: AgentRuntimeKind::OhMyPi,
                role: AgentRole::Worker,
                description: String::new(),
                model: None,
                permission_profile: PermissionProfile::Default,
                max_concurrency: 1,
            persona: Default::default(),
            persona_custom: String::new(),
            })
            .unwrap_err();
        assert!(error.to_string().contains("已移除"));
    }

    #[test]
    fn legacy_active_run_and_task_keep_their_status_during_recovery() {
        let mut fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "旧运行成员");
        let task = fixture.task(&project.id, &agent.id, "运行中任务");
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        let database = fixture.root.join("control.db");
        fixture.store.take();
        {
            let connection = Connection::open(&database).unwrap();
            connection
                .execute(
                    "UPDATE ma_agents SET runtime_kind='oh_my_pi' WHERE id=?1",
                    [&agent.id],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE ma_tasks SET status='running' WHERE id=?1",
                    [&task.id],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE ma_runs SET status='running', lease_expires_at=0 WHERE id=?1",
                    [&run.id],
                )
                .unwrap();
        }
        fixture.store = Some(MultiAgentStore::open_at(database).unwrap());
        let store = fixture.store();
        assert_eq!(
            store
                .recover_interrupted_runs_at_startup()
                .unwrap()
                .interrupted_runs,
            0
        );
        assert_eq!(store.recover_expired_leases(&project.id).unwrap(), 0);
        let snapshot = store.snapshot(&project.id).unwrap();
        assert_eq!(
            snapshot
                .tasks
                .iter()
                .find(|item| item.id == task.id)
                .unwrap()
                .status,
            TaskStatus::Running
        );
        assert_eq!(
            snapshot
                .runs
                .iter()
                .find(|item| item.id == run.id)
                .unwrap()
                .status,
            RunStatus::Running
        );
    }

    #[test]
    fn version_three_database_is_migrated_with_collaboration_defaults() {
        let root = std::env::temp_dir().join(format!("crewvela-ma-v3-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = root.join("control.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE ma_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 INSERT INTO ma_meta VALUES ('schema_version', '3');
                 CREATE TABLE ma_projects (
                    id TEXT PRIMARY KEY, name TEXT NOT NULL, goal TEXT NOT NULL,
                    root_path TEXT NOT NULL, default_branch TEXT NOT NULL,
                    status TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE ma_agents (
                    id TEXT PRIMARY KEY, project_id TEXT NOT NULL, name TEXT NOT NULL,
                    runtime_kind TEXT NOT NULL, role TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
                    model TEXT, permission_profile TEXT NOT NULL DEFAULT 'default',
                    max_concurrency INTEGER NOT NULL DEFAULT 1, enabled INTEGER NOT NULL DEFAULT 1,
                    created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
                    UNIQUE(project_id, name)
                 );
                 CREATE TABLE ma_tasks (
                    id TEXT PRIMARY KEY, project_id TEXT NOT NULL, parent_id TEXT, stage INTEGER NOT NULL DEFAULT 0,
                    title TEXT NOT NULL, description TEXT NOT NULL DEFAULT '', acceptance_criteria TEXT NOT NULL DEFAULT '',
                    verification_command TEXT, status TEXT NOT NULL, assignee_agent_id TEXT, reviewer_agent_id TEXT,
                    workspace_mode TEXT NOT NULL DEFAULT 'worktree', priority INTEGER NOT NULL DEFAULT 0,
                    created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE ma_runs (
                    id TEXT PRIMARY KEY, project_id TEXT NOT NULL, task_id TEXT NOT NULL, agent_id TEXT NOT NULL,
                    attempt INTEGER NOT NULL, status TEXT NOT NULL, session_locator_json TEXT,
                    lease_expires_at INTEGER NOT NULL, output_summary TEXT, error TEXT,
                    created_at INTEGER NOT NULL, started_at INTEGER, finished_at INTEGER,
                    UNIQUE(task_id, attempt)
                 );
                 INSERT INTO ma_projects VALUES ('project', 'legacy', 'migrate', '.', 'main', 'draft', 1, 1);
                 INSERT INTO ma_agents VALUES ('agent', 'project', 'Codex', 'codex', 'worker', '', NULL, 'auto', 1, 1, 1, 1);
                 INSERT INTO ma_tasks VALUES ('task', 'project', NULL, 0, 'legacy task', '', '', NULL, 'ready', 'agent', NULL, 'worktree', 0, 1, 1);
                 INSERT INTO ma_runs VALUES ('run', 'project', 'task', 'agent', 1, 'queued', NULL, 1000, NULL, NULL, 1, NULL, NULL);",
            )
            .unwrap();
        drop(connection);

        let store = MultiAgentStore::open_at(database).unwrap();
        let snapshot = store.snapshot("project").unwrap();
        assert!(snapshot.agents[0]
            .capabilities
            .contains(&"shell".to_string()));
        assert!(snapshot.agents[0].capabilities.contains(&"mcp".to_string()));
        assert!(snapshot.tasks[0].required_capabilities.is_empty());
        assert_eq!(snapshot.tasks[0].max_attempts, 3);
        assert_eq!(snapshot.tasks[0].retry_backoff_seconds, 5);
        assert_eq!(snapshot.runs[0].last_heartbeat_at, None);
        let version: String = store
            .connection
            .lock()
            .unwrap()
            .query_row(
                "SELECT value FROM ma_meta WHERE key = 'schema_version'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION.to_string());
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn create_project_creates_a_missing_workspace_directory() {
        let fixture = TestStore::new();
        let workspace = fixture.root.join("new").join("nested-workspace");

        let project = fixture
            .store()
            .create_project(CreateProjectInput {
                name: "自动创建目录".to_string(),
                goal: "验证项目确认流程".to_string(),
                root_path: workspace.display().to_string(),
                default_branch: "main".to_string(),
            })
            .unwrap();

        assert!(workspace.is_dir());
        assert_eq!(
            PathBuf::from(project.root_path),
            workspace.canonicalize().unwrap()
        );
    }

    #[test]
    fn clearing_coordinator_messages_preserves_agent_history() {
        let fixture = TestStore::new();
        let project = fixture.project();
        fixture
            .store()
            .record_message(&project.id, "user", "开发登录页面")
            .unwrap();
        fixture
            .store()
            .record_message(&project.id, "coordinator", "计划已确认")
            .unwrap();
        fixture
            .store()
            .record_message(&project.id, "agent", "实现完成")
            .unwrap();

        let deleted = fixture
            .store()
            .clear_coordinator_messages(&project.id)
            .unwrap();
        let snapshot = fixture.store().snapshot(&project.id).unwrap();

        assert_eq!(deleted, 2);
        assert_eq!(snapshot.messages.len(), 1);
        assert_eq!(snapshot.messages[0].sender_type, "agent");
        assert_eq!(snapshot.messages[0].body, "实现完成");
    }

    #[test]
    fn task_handoff_blocks_claims_and_preserves_context_and_owner_on_retry() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let source = fixture.agent(&project.id, "原成员");
        let target = fixture.agent(&project.id, "接收成员");
        let task = fixture.task(&project.id, &source.id, "交接测试");
        fixture
            .store()
            .begin_task_handoff(&task.id, "handoff-test")
            .unwrap();
        assert!(fixture
            .store()
            .set_project_status(&project.id, ProjectStatus::Active)
            .is_err());
        assert!(fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: source.id.clone(),
                lease_seconds: 60
            })
            .is_err());
        fixture
            .store()
            .set_project_status(&project.id, ProjectStatus::Paused)
            .unwrap();
        let transferred = fixture
            .store()
            .finish_task_handoff(
                &task.id,
                &target.id,
                Some(&source.id),
                "handoff-test",
                "目标/剩余/阻塞/验证未执行",
            )
            .unwrap();
        assert_eq!(
            transferred.assignee_agent_id.as_deref(),
            Some(target.id.as_str())
        );
        assert_eq!(
            fixture.store().project(&project.id).unwrap().status,
            ProjectStatus::Paused
        );
        let unrelated = fixture.task(&project.id, &target.id, "另一项任务");
        assert!(fixture.store().claim_agent_instructions(&project.id, &target.id, &unrelated.id).unwrap().is_empty());
        assert_eq!(
            fixture
                .store()
                .claim_agent_instructions(&project.id, &target.id, &task.id)
                .unwrap(),
            ["目标/剩余/阻塞/验证未执行"]
        );
        // 模拟最终草稿保存前退出：已完成交接的 begin/finish 不能重新留下锁或指令。
        fixture
            .store()
            .begin_task_handoff(&task.id, "handoff-test")
            .unwrap();
        fixture
            .store()
            .finish_task_handoff(
                &task.id,
                &target.id,
                Some(&source.id),
                "handoff-test",
                "不得重复",
            )
            .unwrap();
        assert!(!fixture.store().handoff_in_progress(&project.id).unwrap());
        assert!(fixture
            .store()
            .task_handoff_completed("handoff-test")
            .unwrap());
        assert!(fixture
            .store()
            .claim_agent_instructions(&project.id, &target.id, &task.id)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn task_handoff_refuses_active_run_and_stale_owner() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let source = fixture.agent(&project.id, "原成员");
        let target = fixture.agent(&project.id, "接收成员");
        let task = fixture.task(&project.id, &source.id, "运行中交接");
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: source.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .begin_task_handoff(&task.id, "active-handoff")
            .unwrap();
        fixture
            .store()
            .set_project_status(&project.id, ProjectStatus::Paused)
            .unwrap();
        assert!(fixture
            .store()
            .finish_task_handoff(
                &task.id,
                &target.id,
                Some(&source.id),
                "active-handoff",
                "包"
            )
            .is_err());
        assert_eq!(
            fixture.store().snapshot(&project.id).unwrap().tasks[0]
                .assignee_agent_id
                .as_deref(),
            Some(source.id.as_str())
        );
        fixture.store().pause_run(&run.id).unwrap();
        assert!(fixture
            .store()
            .finish_task_handoff(
                &task.id,
                &target.id,
                Some("stale-owner"),
                "active-handoff",
                "包"
            )
            .is_err());
        fixture
            .store()
            .release_handoff_lock(&project.id, "other-id")
            .unwrap();
        assert!(fixture.store().handoff_in_progress(&project.id).unwrap());
        fixture
            .store()
            .release_handoff_lock(&project.id, "active-handoff")
            .unwrap();
        assert!(!fixture.store().handoff_in_progress(&project.id).unwrap());
        assert_eq!(
            fixture.store().project(&project.id).unwrap().status,
            ProjectStatus::Paused
        );
    }

    #[test]
    fn acp_execution_marker_cannot_start_after_handoff_lock() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let source = fixture.agent(&project.id, "成员");
        let task = fixture.task(&project.id, &source.id, "ACP 标记");
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: source.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        fixture.store().mark_acp_execution(&run.id).unwrap();
        assert_eq!(
            fixture.store().snapshot(&project.id).unwrap().runs[0]
                .session_locator
                .as_ref()
                .unwrap()["transport"],
            "acp"
        );
        fixture.store().clear_acp_execution(&run.id).unwrap();
        fixture.store().bind_pty_execution(&run.id, 42).unwrap();
        let locator = fixture.store().snapshot(&project.id).unwrap().runs[0]
            .session_locator
            .clone()
            .unwrap();
        assert_eq!(locator["transport"], "pty");
        assert_eq!(locator["ptyId"], 42);
        fixture
            .store()
            .begin_task_handoff(&task.id, "block-acp")
            .unwrap();
        assert!(fixture.store().mark_acp_execution(&run.id).is_err());
        assert!(fixture.store().clear_acp_execution(&run.id).is_err());
        assert!(fixture.store().bind_pty_execution(&run.id, 43).is_err());
    }

    #[test]
    fn pausing_a_run_requeues_its_task_without_losing_the_project() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "可恢复任务");
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: Some(serde_json::json!({"cwd": fixture.root})),
            })
            .unwrap();

        let paused = fixture.store().pause_run(&run.id).unwrap();
        let snapshot = fixture.store().snapshot(&project.id).unwrap();

        assert_eq!(paused.status, RunStatus::Interrupted);
        assert_eq!(snapshot.tasks[0].status, TaskStatus::Ready);
        assert_eq!(snapshot.project.status, ProjectStatus::Active);
        assert_eq!(
            snapshot.agent_sessions[0].status,
            AgentSessionStatus::Stopped
        );
    }

    #[test]
    fn task_dependency_graph_rejects_cycles() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let first = fixture.task(&project.id, &agent.id, "A");
        let second = fixture.task(&project.id, &agent.id, "B");

        fixture
            .store()
            .add_dependency(AddDependencyInput {
                task_id: second.id.clone(),
                depends_on_task_id: first.id.clone(),
            })
            .unwrap();
        let error = fixture
            .store()
            .add_dependency(AddDependencyInput {
                task_id: first.id,
                depends_on_task_id: second.id,
            })
            .unwrap_err();

        assert!(error.to_string().contains("循环"));
    }

    #[test]
    fn a_task_can_only_have_one_active_claim() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "实现功能");

        fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        let second = fixture.store().claim_task(ClaimTaskInput {
            task_id: task.id,
            agent_id: agent.id,
            lease_seconds: 60,
        });

        assert!(second.is_err());
    }

    #[test]
    fn recovery_blocks_interrupted_write_tasks() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "修改文件");
        fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();

        let receipt = fixture.store().recover_interrupted_runs().unwrap();
        let snapshot = fixture.store().snapshot(&project.id).unwrap();

        assert_eq!(receipt.interrupted_runs, 1);
        assert_eq!(snapshot.tasks[0].status, TaskStatus::Blocked);
        assert_eq!(snapshot.runs[0].status, RunStatus::Interrupted);
        assert_eq!(
            snapshot.agent_sessions[0].status,
            AgentSessionStatus::Stopped
        );
    }

    #[test]
    fn recovery_allows_an_interrupted_write_task_to_be_retried_after_its_wakeup_was_claimed() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "修改文件");

        assert_eq!(
            fixture
                .store()
                .ensure_runnable_wakeups(&project.id)
                .unwrap(),
            1
        );
        let stale_wakeup = fixture
            .store()
            .claim_task_wakeup(&task.id)
            .unwrap()
            .unwrap();
        fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();

        fixture.store().recover_interrupted_runs().unwrap();

        let recovered = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(recovered.tasks[0].status, TaskStatus::Blocked);
        assert_eq!(
            recovered
                .wakeups
                .iter()
                .find(|wakeup| wakeup.id == stale_wakeup.id)
                .unwrap()
                .status,
            "cancelled"
        );
        fixture.store().retry_task(&task.id).unwrap();
        assert_eq!(
            fixture
                .store()
                .ensure_runnable_wakeups(&project.id)
                .unwrap(),
            1
        );
        let retry_wakeup = fixture
            .store()
            .claim_task_wakeup(&task.id)
            .unwrap()
            .unwrap();
        assert_ne!(retry_wakeup.id, stale_wakeup.id);
    }

    #[test]
    fn retry_becomes_runnable_at_the_same_millisecond_as_its_wakeup() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let store = fixture.store();
        let task = fixture.task(&project.id, &agent.id, "重新审查");
        let retry_at = (now_millis() / 1000 + 60) * 1000 + 270;
        store.connection.lock().unwrap().execute(
            "UPDATE ma_tasks SET next_retry_at = ?2 WHERE id = ?1", params![task.id, retry_at],
        ).unwrap();

        assert!(store.runnable_tasks_at(&project.id, retry_at - 1).unwrap().is_empty());
        assert_eq!(store.ensure_runnable_wakeups_at(&project.id, retry_at - 1).unwrap(), 0);
        let runnable = store.runnable_tasks_at(&project.id, retry_at).unwrap();
        assert_eq!(runnable.len(), 1);
        assert_eq!(runnable[0].0.id, task.id);
        assert_eq!(store.ensure_runnable_wakeups_at(&project.id, retry_at).unwrap(), 1);
        assert!(store.claim_task_wakeup(&task.id).unwrap().is_some());
    }

    #[test]
    fn recovery_cancels_a_claimed_wakeup_left_by_an_already_finished_run() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Claude");
        let task = fixture.task(&project.id, &agent.id, "修改页面");

        fixture
            .store()
            .ensure_runnable_wakeups(&project.id)
            .unwrap();
        let stale_wakeup = fixture
            .store()
            .claim_task_wakeup(&task.id)
            .unwrap()
            .unwrap();
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture.store().pause_run(&run.id).unwrap();

        let receipt = fixture
            .store()
            .recover_interrupted_runs_at_startup()
            .unwrap();

        assert_eq!(receipt.interrupted_runs, 0);
        let recovered = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(recovered.tasks[0].status, TaskStatus::Ready);
        assert_eq!(
            recovered
                .wakeups
                .iter()
                .find(|wakeup| wakeup.id == stale_wakeup.id)
                .unwrap()
                .status,
            "cancelled"
        );
        assert_eq!(
            fixture
                .store()
                .ensure_runnable_wakeups(&project.id)
                .unwrap(),
            1
        );
    }

    #[test]
    fn recovery_preserves_a_new_claim_before_its_run_is_created() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Claude");
        let task = fixture.task(&project.id, &agent.id, "继续修改页面");

        fixture
            .store()
            .ensure_runnable_wakeups(&project.id)
            .unwrap();
        fixture
            .store()
            .claim_task_wakeup(&task.id)
            .unwrap()
            .unwrap();
        let first_run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture.store().pause_run(&first_run.id).unwrap();
        fixture
            .store()
            .recover_interrupted_runs_at_startup()
            .unwrap();
        fixture
            .store()
            .ensure_runnable_wakeups(&project.id)
            .unwrap();
        let new_wakeup = fixture
            .store()
            .claim_task_wakeup(&task.id)
            .unwrap()
            .unwrap();

        fixture.store().recover_interrupted_runs().unwrap();

        let recovered = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(
            recovered
                .wakeups
                .iter()
                .find(|wakeup| wakeup.id == new_wakeup.id)
                .unwrap()
                .status,
            "claimed"
        );
    }

    #[test]
    fn unverified_success_waits_for_human_approval() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "实现功能");
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();

        fixture
            .store()
            .finalize_successful_run(&run.id, "worktree", "已实现", false, Some("abc123"), None)
            .unwrap();
        let pending = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(pending.tasks[0].status, TaskStatus::InReview);
        assert_eq!(pending.approvals[0].status, "pending");
        assert_eq!(pending.artifacts[0].content_hash.as_deref(), Some("abc123"));

        fixture.store().approve_task(&task.id).unwrap();
        let approved = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(approved.tasks[0].status, TaskStatus::Done);
        assert_eq!(approved.approvals[0].status, "approved");
    }

    #[test]
    fn scheduler_wakeup_is_durable_and_claimed_once() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "待调度任务");

        assert_eq!(
            fixture
                .store()
                .ensure_runnable_wakeups(&project.id)
                .unwrap(),
            1
        );
        assert_eq!(
            fixture
                .store()
                .ensure_runnable_wakeups(&project.id)
                .unwrap(),
            0
        );
        let wakeup = fixture
            .store()
            .claim_task_wakeup(&task.id)
            .unwrap()
            .unwrap();
        assert!(fixture
            .store()
            .claim_task_wakeup(&task.id)
            .unwrap()
            .is_none());
        fixture.store().finish_wakeup(&wakeup.id, true).unwrap();

        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(snapshot.wakeups[0].status, "completed");
    }

    #[test]
    fn claimed_agent_session_and_live_logs_are_persisted_in_snapshot() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "实现后端接口");
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();

        fixture
            .store()
            .append_run_log(&run.id, "stdout", "正在修改 API")
            .unwrap();
        fixture
            .store()
            .link_agent_app_session(&project.id, &agent.id, "app-session-codex")
            .unwrap();
        let snapshot = fixture.store().snapshot(&project.id).unwrap();

        assert_eq!(snapshot.agent_sessions.len(), 1);
        assert_eq!(
            snapshot.agent_sessions[0].current_run_id.as_deref(),
            Some(run.id.as_str())
        );
        assert_eq!(
            snapshot.agent_sessions[0].current_task_id.as_deref(),
            Some(task.id.as_str())
        );
        assert_eq!(snapshot.run_logs.len(), 1);
        assert_eq!(snapshot.run_logs[0].content, "正在修改 API");
        assert_eq!(snapshot.run_logs[0].agent_id, agent.id);
        assert_eq!(
            snapshot.agent_sessions[0].app_session_id.as_deref(),
            Some("app-session-codex")
        );
        let after = fixture
            .store()
            .run_logs(&project.id, Some(snapshot.run_logs[0].seq), 100)
            .unwrap();
        assert!(after.is_empty());
    }

    #[test]
    fn visible_agent_session_can_be_linked_before_the_first_task_is_claimed() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Claude");

        fixture
            .store()
            .link_agent_app_session(&project.id, &agent.id, "prewarmed-session")
            .unwrap();

        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(snapshot.agent_sessions.len(), 1);
        assert_eq!(
            snapshot.agent_sessions[0].app_session_id.as_deref(),
            Some("prewarmed-session")
        );
        assert_eq!(snapshot.agent_sessions[0].status, AgentSessionStatus::Idle);
    }

    #[test]
    fn dependency_artifacts_include_transitive_implementation_handoffs() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let implementation = fixture.task(&project.id, &agent.id, "实现后端");
        let review = fixture.task(&project.id, &agent.id, "审查后端");
        let integration = fixture.task(&project.id, &agent.id, "集成交付");
        fixture
            .store()
            .add_dependency(AddDependencyInput {
                task_id: review.id.clone(),
                depends_on_task_id: implementation.id.clone(),
            })
            .unwrap();
        fixture
            .store()
            .add_dependency(AddDependencyInput {
                task_id: integration.id.clone(),
                depends_on_task_id: review.id.clone(),
            })
            .unwrap();

        for (task, uri) in [
            (&implementation, "implementation-worktree"),
            (&review, "review-workspace"),
        ] {
            let run = fixture
                .store()
                .claim_task(ClaimTaskInput {
                    task_id: task.id.clone(),
                    agent_id: agent.id.clone(),
                    lease_seconds: 60,
                })
                .unwrap();
            fixture
                .store()
                .update_run(UpdateRunInput {
                    run_id: run.id.clone(),
                    status: RunStatus::Running,
                    output_summary: None,
                    error: None,
                    session_locator: None,
                })
                .unwrap();
            fixture
                .store()
                .finalize_successful_run(&run.id, uri, "完成", true, Some(uri), None)
                .unwrap();
        }

        let artifacts = fixture
            .store()
            .dependency_artifacts(&integration.id)
            .unwrap();
        assert_eq!(artifacts.len(), 2);
        assert!(artifacts
            .iter()
            .any(|artifact| artifact.task_id == implementation.id));
        assert!(artifacts
            .iter()
            .any(|artifact| artifact.task_id == review.id));
    }

    #[test]
    fn agent_concurrency_is_clamped_to_one_visible_session() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture
            .store()
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: project.id,
                name: "并发 Agent".to_string(),
                runtime_kind: AgentRuntimeKind::Codex,
                role: AgentRole::Worker,
                description: String::new(),
                model: None,
                permission_profile: PermissionProfile::Auto,
                max_concurrency: 8,
            persona: Default::default(),
            persona_custom: String::new(),
            })
            .unwrap();
        assert_eq!(agent.max_concurrency, 1);
    }

    #[test]
    fn manual_takeover_pauses_future_claims_and_delivers_instruction_once() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Claude");
        let task = fixture.task(&project.id, &agent.id, "实现前端页面");

        assert_eq!(
            fixture.store().runnable_tasks(&project.id).unwrap().len(),
            1
        );
        fixture
            .store()
            .set_agent_control(
                &project.id,
                &agent.id,
                AgentControlMode::Manual,
                Some("优先检查接口类型"),
            )
            .unwrap();
        assert!(fixture
            .store()
            .runnable_tasks(&project.id)
            .unwrap()
            .is_empty());
        assert!(fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .is_err());

        let instructions = fixture
            .store()
            .claim_agent_instructions(&project.id, &agent.id, &task.id)
            .unwrap();
        assert_eq!(instructions, vec!["优先检查接口类型"]);
        assert!(fixture
            .store()
            .claim_agent_instructions(&project.id, &agent.id, &task.id)
            .unwrap()
            .is_empty());

        fixture
            .store()
            .set_agent_control(&project.id, &agent.id, AgentControlMode::Auto, None)
            .unwrap();
        assert_eq!(
            fixture.store().runnable_tasks(&project.id).unwrap().len(),
            1
        );
    }

    #[test]
    fn explicit_reviewer_task_finishes_without_a_second_manual_gate() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let implementer = fixture.agent(&project.id, "Claude");
        let implementation = fixture.task(&project.id, &implementer.id, "实现功能");
        let implementation_run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: implementation.id.clone(),
                agent_id: implementer.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: implementation_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store()
            .finalize_successful_run(
                &implementation_run.id,
                "implementation",
                "实现完成",
                true,
                Some("implementation-hash"),
                None,
            )
            .unwrap();
        let reviewer = fixture
            .store()
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: project.id.clone(),
                name: "Codex Review".to_string(),
                runtime_kind: AgentRuntimeKind::Codex,
                role: AgentRole::Reviewer,
                description: "审查前后端代码".to_string(),
                model: None,
                permission_profile: PermissionProfile::Auto,
                max_concurrency: 1,
            persona: Default::default(),
            persona_custom: String::new(),
            })
            .unwrap();
        let task = fixture.task(&project.id, &reviewer.id, "交叉 Review");
        fixture
            .store()
            .add_dependency(AddDependencyInput {
                task_id: task.id.clone(),
                depends_on_task_id: implementation.id.clone(),
            })
            .unwrap();
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: reviewer.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .submit_review_decision(SubmitReviewDecisionInput {
                review_run_id: run.id.clone(),
                target_task_id: implementation.id.clone(),
                verdict: ReviewVerdict::Approved,
                summary: "代码和测试均通过".to_string(),
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store()
            .finalize_successful_run(
                &run.id,
                "review",
                "审查完成",
                false,
                Some("review-hash"),
                None,
            )
            .unwrap();

        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(
            snapshot
                .tasks
                .iter()
                .find(|item| item.id == task.id)
                .unwrap()
                .status,
            TaskStatus::Done
        );
        assert!(snapshot.approvals.is_empty());
        assert_eq!(snapshot.review_decisions.len(), 1);
        assert!(snapshot.review_decisions[0].applied_at.is_some());
    }

    #[test]
    fn review_changes_request_reopens_implementation_then_approved_review_unblocks_delivery() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let implementer = fixture.agent(&project.id, "Claude");
        let reviewer = fixture
            .store()
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: project.id.clone(),
                name: "Codex Review".to_string(),
                runtime_kind: AgentRuntimeKind::Codex,
                role: AgentRole::Reviewer,
                description: "审查实现与测试".to_string(),
                model: None,
                permission_profile: PermissionProfile::Auto,
                max_concurrency: 1,
            persona: Default::default(),
            persona_custom: String::new(),
            })
            .unwrap();
        let implementation = fixture.task(&project.id, &implementer.id, "实现功能");
        let review = fixture
            .store()
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 1,
                title: "审核功能".to_string(),
                description: "检查实现和测试".to_string(),
                acceptance_criteria: "提交结构化审核结论".to_string(),
                verification_command: None,
                assignee_agent_id: Some(reviewer.id.clone()),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::ReadOnly,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 3,
                retry_backoff_seconds: 0,
            })
            .unwrap();
        let delivery = fixture.task(&project.id, &implementer.id, "集成交付");
        fixture
            .store()
            .add_dependency(AddDependencyInput {
                task_id: review.id.clone(),
                depends_on_task_id: implementation.id.clone(),
            })
            .unwrap();
        fixture
            .store()
            .add_dependency(AddDependencyInput {
                task_id: delivery.id.clone(),
                depends_on_task_id: review.id.clone(),
            })
            .unwrap();

        let implementation_run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: implementation.id.clone(),
                agent_id: implementer.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: implementation_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store()
            .finalize_successful_run(
                &implementation_run.id,
                "implementation-v1",
                "首版实现",
                true,
                Some("implementation-v1"),
                None,
            )
            .unwrap();

        let review_run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: review.id.clone(),
                agent_id: reviewer.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: review_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        let rejected = SubmitReviewDecisionInput {
            review_run_id: review_run.id.clone(),
            target_task_id: implementation.id.clone(),
            verdict: ReviewVerdict::ChangesRequested,
            summary: "缺少失败分支测试，请补充后重新审核".to_string(),
        };
        let first_decision = fixture
            .store()
            .submit_review_decision(rejected.clone())
            .unwrap();
        let duplicate = fixture.store().submit_review_decision(rejected).unwrap();
        assert_eq!(duplicate.id, first_decision.id);
        assert!(fixture
            .store()
            .submit_review_decision(SubmitReviewDecisionInput {
                review_run_id: review_run.id.clone(),
                target_task_id: implementation.id.clone(),
                verdict: ReviewVerdict::Approved,
                summary: "改成通过".to_string(),
            })
            .is_err());
        let old_review_locator = serde_json::json!({"cwd": "stale-review-worktree"});
        fixture
            .store()
            .finalize_successful_run(
                &review_run.id,
                "review-v1",
                "发现测试缺失",
                true,
                Some("review-v1"),
                Some(&old_review_locator),
            )
            .unwrap();

        let rejected_snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(
            rejected_snapshot
                .tasks
                .iter()
                .find(|task| task.id == implementation.id)
                .unwrap()
                .status,
            TaskStatus::ChangesRequested
        );
        assert_eq!(
            rejected_snapshot
                .tasks
                .iter()
                .find(|task| task.id == review.id)
                .unwrap()
                .status,
            TaskStatus::Blocked
        );
        assert_eq!(
            rejected_snapshot
                .tasks
                .iter()
                .find(|task| task.id == delivery.id)
                .unwrap()
                .status,
            TaskStatus::Blocked
        );
        assert!(rejected_snapshot.messages.iter().any(|message| {
            message.task_id.as_deref() == Some(implementation.id.as_str())
                && message.target_agent_id.as_deref() == Some(implementer.id.as_str())
                && message.body.contains("失败分支测试")
        }));
        assert!(fixture
            .store()
            .latest_session_locator(&review.id)
            .unwrap()
            .is_none());

        let feedback = fixture
            .store()
            .scoped_messages(&project.id, &implementation.id, &implementer.id, 20)
            .unwrap();
        assert!(feedback
            .iter()
            .any(|message| message.body.contains("失败分支测试")));
        let repair_run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: implementation.id.clone(),
                agent_id: implementer.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        assert_eq!(repair_run.attempt, 2);
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: repair_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store()
            .finalize_successful_run(
                &repair_run.id,
                "implementation-v2",
                "已补充失败分支测试",
                true,
                Some("implementation-v2"),
                None,
            )
            .unwrap();
        assert_eq!(
            fixture
                .store()
                .snapshot(&project.id)
                .unwrap()
                .tasks
                .iter()
                .find(|task| task.id == review.id)
                .unwrap()
                .status,
            TaskStatus::Ready
        );

        let second_review_run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: review.id.clone(),
                agent_id: reviewer.id,
                lease_seconds: 60,
            })
            .unwrap();
        assert_eq!(second_review_run.attempt, 2);
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: second_review_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store()
            .submit_review_decision(SubmitReviewDecisionInput {
                review_run_id: second_review_run.id.clone(),
                target_task_id: implementation.id,
                verdict: ReviewVerdict::Approved,
                summary: "修复和测试均通过".to_string(),
            })
            .unwrap();
        fixture
            .store()
            .finalize_successful_run(
                &second_review_run.id,
                "review-v2",
                "复审通过",
                true,
                Some("review-v2"),
                None,
            )
            .unwrap();

        let approved_snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(
            approved_snapshot
                .tasks
                .iter()
                .find(|task| task.id == review.id)
                .unwrap()
                .status,
            TaskStatus::Done
        );
        assert_eq!(
            approved_snapshot
                .tasks
                .iter()
                .find(|task| task.id == delivery.id)
                .unwrap()
                .status,
            TaskStatus::Ready
        );
        assert_eq!(approved_snapshot.review_decisions.len(), 2);
        assert!(approved_snapshot
            .events
            .iter()
            .any(|event| event.event_type == "review.changes_requested"));
        assert!(approved_snapshot
            .events
            .iter()
            .any(|event| event.event_type == "review.approved"));
    }

    #[test]
    fn reviewer_completion_without_decision_is_rejected_atomically() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let implementer = fixture.agent(&project.id, "Claude");
        let implementation = fixture.task(&project.id, &implementer.id, "已完成实现");
        let implementation_run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: implementation.id.clone(),
                agent_id: implementer.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: implementation_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store()
            .finalize_successful_run(
                &implementation_run.id,
                "implementation",
                "完成",
                true,
                Some("hash"),
                None,
            )
            .unwrap();
        let reviewer = fixture
            .store()
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: project.id.clone(),
                name: "Reviewer".to_string(),
                runtime_kind: AgentRuntimeKind::Codex,
                role: AgentRole::Reviewer,
                description: String::new(),
                model: None,
                permission_profile: PermissionProfile::Auto,
                max_concurrency: 1,
            persona: Default::default(),
            persona_custom: String::new(),
            })
            .unwrap();
        let review = fixture.task(&project.id, &reviewer.id, "审核");
        fixture
            .store()
            .add_dependency(AddDependencyInput {
                task_id: review.id.clone(),
                depends_on_task_id: implementation.id,
            })
            .unwrap();
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: review.id.clone(),
                agent_id: reviewer.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();

        assert!(fixture
            .store()
            .finalize_successful_run(&run.id, "review", "未提交结论", true, None, None)
            .is_err());
        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(
            snapshot
                .runs
                .iter()
                .find(|item| item.id == run.id)
                .unwrap()
                .status,
            RunStatus::Running
        );
        assert_eq!(
            snapshot
                .tasks
                .iter()
                .find(|task| task.id == review.id)
                .unwrap()
                .status,
            TaskStatus::Running
        );
        assert!(snapshot
            .artifacts
            .iter()
            .all(|artifact| artifact.run_id != run.id));
    }

    #[test]
    fn permanent_failure_exhausts_retry_immediately() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "OpenCode");
        let task = fixture
            .store()
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "打招呼".to_string(),
                description: String::new(),
                acceptance_criteria: "送达".to_string(),
                verification_command: None,
                assignee_agent_id: Some(agent.id.clone()),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::SharedWrite,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 3,
                retry_backoff_seconds: 5,
            })
            .unwrap();
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();

        fixture
            .store()
            .fail_run_permanently(&run.id, "配置错误：Auto 权限无法隔离", None)
            .unwrap();

        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        let task = snapshot
            .tasks
            .into_iter()
            .find(|item| item.id == task.id)
            .unwrap();
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(task.next_retry_at, None, "永久性错误不应安排重试");
        let run = snapshot
            .runs
            .into_iter()
            .find(|item| item.id == run.id)
            .unwrap();
        assert_eq!(run.status, RunStatus::Failed);
    }

    #[test]
    fn dispatch_message_is_recorded_only_on_the_first_attempt() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "OpenCode");
        let task = fixture.task(&project.id, &agent.id, "打招呼");
        let first = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .fail_run_with_retry(&first.id, "暂时失败", None)
            .unwrap();
        // Advance this temporary database past the retry deadline without sleeping.
        fixture.store().connection.lock().unwrap().execute(
            "UPDATE ma_tasks SET next_retry_at=?1 WHERE id=?2",
            params![now_millis() - 1, task.id],
        ).unwrap();
        fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id.clone(),
                lease_seconds: 60,
            })
            .unwrap();

        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        let dispatches = snapshot
            .messages
            .iter()
            .filter(|message| message.body.starts_with("派发任务"))
            .count();
        assert_eq!(dispatches, 1, "重试不应重复写入派发消息");
    }

    #[test]
    fn review_loop_stops_when_repair_attempt_budget_is_exhausted() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let implementer = fixture.agent(&project.id, "Claude");
        let implementation = fixture
            .store()
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "仅允许一次实现".to_string(),
                description: String::new(),
                acceptance_criteria: "测试通过".to_string(),
                verification_command: None,
                assignee_agent_id: Some(implementer.id.clone()),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::Worktree,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 1,
                retry_backoff_seconds: 0,
            })
            .unwrap();
        let reviewer = fixture
            .store()
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: project.id.clone(),
                name: "Reviewer".to_string(),
                runtime_kind: AgentRuntimeKind::Codex,
                role: AgentRole::Reviewer,
                description: String::new(),
                model: None,
                permission_profile: PermissionProfile::Auto,
                max_concurrency: 1,
            persona: Default::default(),
            persona_custom: String::new(),
            })
            .unwrap();
        let review = fixture.task(&project.id, &reviewer.id, "审核");
        fixture
            .store()
            .add_dependency(AddDependencyInput {
                task_id: review.id.clone(),
                depends_on_task_id: implementation.id.clone(),
            })
            .unwrap();

        let implementation_run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: implementation.id.clone(),
                agent_id: implementer.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: implementation_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store()
            .finalize_successful_run(
                &implementation_run.id,
                "implementation",
                "完成",
                true,
                Some("hash"),
                None,
            )
            .unwrap();
        let review_run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: review.id.clone(),
                agent_id: reviewer.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: review_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store()
            .submit_review_decision(SubmitReviewDecisionInput {
                review_run_id: review_run.id.clone(),
                target_task_id: implementation.id.clone(),
                verdict: ReviewVerdict::ChangesRequested,
                summary: "必须修复".to_string(),
            })
            .unwrap();
        fixture
            .store()
            .finalize_successful_run(
                &review_run.id,
                "review",
                "退回",
                true,
                Some("review-hash"),
                None,
            )
            .unwrap();

        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(
            snapshot
                .tasks
                .iter()
                .find(|task| task.id == implementation.id)
                .unwrap()
                .status,
            TaskStatus::Failed
        );
        assert_eq!(
            snapshot
                .tasks
                .iter()
                .find(|task| task.id == review.id)
                .unwrap()
                .status,
            TaskStatus::Failed
        );
        assert!(snapshot
            .events
            .iter()
            .any(|event| event.event_type == "review.loop_exhausted"));
    }

    #[test]
    fn unverified_stop_survives_lease_expiry_and_restart_without_retry() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "停止未确认");
        let run = fixture.store().claim_task(ClaimTaskInput { task_id: task.id.clone(), agent_id: agent.id.clone(), lease_seconds: 30 }).unwrap();
        fixture.store().mark_run_stop_unverified(&run.id, "原进程停止超时").unwrap();
        fixture.store().connection.lock().unwrap().execute("UPDATE ma_runs SET lease_expires_at = 0 WHERE id = ?1", [&run.id]).unwrap();
        let reopened = MultiAgentStore::open_at(fixture.root.join("control.db")).unwrap();
        assert_eq!(reopened.recover_expired_leases(&project.id).unwrap(), 0);
        reopened.recover_interrupted_runs_at_startup().unwrap();
        assert!(reopened.fail_run_with_retry(&run.id, "expired", None).is_err());
        let snapshot = reopened.snapshot(&project.id).unwrap();
        assert!(snapshot.runs.iter().any(|item| item.id == run.id && !is_terminal_run_status(item.status)));
        assert_eq!(snapshot.tasks.iter().find(|item| item.id == task.id).unwrap().status, TaskStatus::Blocked);
        assert!(reopened.claim_task(ClaimTaskInput { task_id: task.id.clone(), agent_id: agent.id.clone(), lease_seconds: 30 }).is_err());
        assert!(reopened.confirm_run_stopped(&run.id, false).is_err());
        assert!(reopened.cancel_project_tasks(&project.id).is_err());
        let confirmed = reopened.confirm_run_stopped(&run.id, true).unwrap();
        assert_eq!(confirmed.status, RunStatus::Cancelled);
        let ready = reopened.snapshot(&project.id).unwrap();
        assert_eq!(ready.tasks.iter().find(|item| item.id == task.id).unwrap().status, TaskStatus::Ready);
        drop(reopened);
    }

    #[test]
    fn ordinary_expired_lease_still_allows_bounded_retry() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "普通租约");
        let run = fixture.store().claim_task(ClaimTaskInput { task_id: task.id.clone(), agent_id: agent.id, lease_seconds: 30 }).unwrap();
        fixture.store().connection.lock().unwrap().execute("UPDATE ma_runs SET lease_expires_at = 0 WHERE id = ?1", [&run.id]).unwrap();
        assert_eq!(fixture.store().recover_expired_leases(&project.id).unwrap(), 1);
        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(snapshot.tasks.iter().find(|item| item.id == task.id).unwrap().status, TaskStatus::Ready);
    }

    #[test]
    fn heartbeat_renews_the_run_lease_and_records_a_runtime_event() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "长任务");
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id,
                agent_id: agent.id,
                lease_seconds: 30,
            })
            .unwrap();

        let heartbeat = fixture
            .store()
            .heartbeat_run(HeartbeatRunInput {
                run_id: run.id.clone(),
                lease_seconds: 120,
            })
            .unwrap();

        assert!(heartbeat.lease_expires_at > run.lease_expires_at);
        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        let renewed = snapshot.runs.iter().find(|item| item.id == run.id).unwrap();
        assert_eq!(renewed.last_heartbeat_at, Some(heartbeat.heartbeat_at));
        assert!(snapshot
            .runtime_events
            .iter()
            .any(|event| event.run_id == run.id && event.event_type == "heartbeat"));
    }

    #[test]
    fn failed_runs_schedule_bounded_exponential_retry() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture
            .store()
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "可重试任务".to_string(),
                description: String::new(),
                acceptance_criteria: "通过".to_string(),
                verification_command: None,
                assignee_agent_id: Some(agent.id.clone()),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::Worktree,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 2,
                retry_backoff_seconds: 1,
            })
            .unwrap();
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();

        fixture
            .store()
            .fail_run_with_retry(&run.id, "临时失败", None)
            .unwrap();

        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        let task = snapshot
            .tasks
            .iter()
            .find(|item| item.id == task.id)
            .unwrap();
        assert_eq!(task.status, TaskStatus::Ready);
        assert!(task.next_retry_at.is_some());
        assert!(fixture
            .store()
            .runnable_tasks(&project.id)
            .unwrap()
            .is_empty());
        assert!(snapshot.events.iter().any(|event| {
            event.entity_id == run.id && event.event_type == "run.retry_scheduled"
        }));
    }

    #[test]
    fn agent_knowledge_search_hides_private_conversations_and_message_task_sources() {
        let fixture = TestStore::new();
        let store = fixture.store();
        let project = fixture.project();
        let sender = fixture.agent(&project.id, "sender");
        let receiver = fixture.agent(&project.id, "receiver");
        let observer = fixture.agent(&project.id, "observer");
        let sender_task = fixture.task(&project.id, &sender.id, "public business task");
        let observer_task = fixture.task(&project.id, &observer.id, "public observer task");
        let secret = "privatecommunicationmarker";
        store.send_collaboration_message(SendCollaborationMessageInput {
            project_id: project.id.clone(), task_id: Some(sender_task.id.clone()),
            sender_type: "agent".into(), sender_id: Some(sender.id.clone()),
            target_agent_id: Some(receiver.id.clone()), body: secret.into(),
        }).unwrap();
        store.record_message(&project.id, "user", "publiccommunicationmarker shared project context").unwrap();
        assert!(store.enqueue_cli_message_task(&project.id, &receiver.id, "private-route", secret).unwrap());
        let private_task = "cli-message-private-route";
        let run = store.claim_task(ClaimTaskInput {
            task_id: private_task.into(), agent_id: receiver.id.clone(), lease_seconds: 60,
        }).unwrap();
        store.send_collaboration_message(SendCollaborationMessageInput {
            project_id: project.id.clone(), task_id: Some(private_task.into()),
            sender_type: "agent".into(), sender_id: Some(receiver.id.clone()),
            target_agent_id: Some(sender.id.clone()), body: format!("{secret} reply from the isolated message task"),
        }).unwrap();
        store.update_run(UpdateRunInput {
            run_id: run.id.clone(), status: RunStatus::Running,
            output_summary: Some(secret.into()), error: None, session_locator: None,
        }).unwrap();
        store.publish_artifact(PublishArtifactInput {
            project_id: project.id.clone(), task_id: private_task.into(), run_id: run.id,
            artifact_type: "message evidence".into(), uri: "fixture://private".into(),
            summary: secret.into(), content_hash: None,
        }).unwrap();
        store.put_memory(PutMemoryInput {
            project_id: project.id.clone(), namespace: "message".into(), key: "private route".into(),
            value: secret.into(), source_task_id: Some(private_task.into()),
            source_run_id: None, confidence: 1.0,
        }).unwrap();
        // Verification establishes quality, not permission to share a private
        // message task's content through the separate memory API.
        let memory_input = || SearchMemoryInput {
            project_id: project.id.clone(), query: secret.into(), namespace: None,
            current_run_id: None, limit: 100,
        };
        assert_eq!(store.search_memory(memory_input()).unwrap().len(), 1);
        assert_eq!(store.search_memory_for_agent(memory_input(), &receiver.id).unwrap().len(), 1);
        assert!(store.search_memory_for_agent(memory_input(), &observer.id).unwrap().is_empty());
        assert!(store.search_memory_for_agent(memory_input(), &sender.id).unwrap().is_empty());
        for query in ["", secret] {
            for source in [None, Some("conversation"), Some("task"), Some("run"), Some("artifact"), Some("memory")] {
                let input = |offset, limit| SearchKnowledgeInput {
                    project_id: project.id.clone(), query: query.into(),
                    source_types: source.into_iter().map(str::to_string).collect(), task_id: None, offset, limit,
                };
                let admin = store.search_knowledge(input(0, 100)).unwrap();
                assert!(admin.iter().any(|result| result.content.contains(secret)), "admin lost {source:?} source");
                let visible = store.search_knowledge_for_agent(input(0, 100), &observer.id, &observer_task.id).unwrap();
                assert!(visible.iter().all(|result| !result.content.contains(secret)), "private content leaked through {source:?}/{query:?}");
                let received = store.search_knowledge_for_agent(input(0, 100), &receiver.id, private_task).unwrap();
                assert!(received.iter().any(|result| result.content.contains(secret)), "receiver lost {source:?}/{query:?}");
                // Authorization must run before pagination, so hidden rows do
                // not consume slots or cause gaps in the observer's pages.
                let mut pages = Vec::new();
                for offset in 0..visible.len() as u32 {
                    pages.extend(store.search_knowledge_for_agent(input(offset, 1), &observer.id, &observer_task.id).unwrap());
                }
                assert_eq!(pages.iter().map(|row| &row.id).collect::<Vec<_>>(), visible.iter().map(|row| &row.id).collect::<Vec<_>>());
            }
        }
        let sender_view = store.search_knowledge_for_agent(SearchKnowledgeInput {
            project_id: project.id.clone(), query: secret.into(), source_types: vec![], task_id: None, offset: 0, limit: 100,
        }, &sender.id, &sender_task.id).unwrap();
        assert_eq!(sender_view.len(), 2, "sender can read its conversation and received reply, not the receiver's isolated work");
        assert!(sender_view.iter().all(|result| result.source_type == "conversation"));
        let public = store.search_knowledge_for_agent(SearchKnowledgeInput {
            project_id: project.id, query: "publiccommunicationmarker".into(), source_types: vec![], task_id: None, offset: 0, limit: 100,
        }, &observer.id, &observer_task.id).unwrap();
        assert_eq!(public.len(), 1);
    }

    #[test]
    fn knowledge_index_searches_project_tasks_and_conversations_with_citations() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Claude");
        let task = fixture.task(&project.id, &agent.id, "实时语音打断修复");
        fixture
            .store()
            .record_message(
                &project.id,
                "user",
                "用户讲话时终止 TTS 播放，并保留完整朗读。",
            )
            .unwrap();

        let receipt = fixture
            .store()
            .rebuild_knowledge_index(&project.id)
            .unwrap();
        assert!(receipt.document_count >= 3);
        assert_eq!(receipt.source_counts.get("task"), Some(&1));
        assert_eq!(receipt.source_counts.get("conversation"), Some(&1));

        let tasks = fixture
            .store()
            .search_knowledge(SearchKnowledgeInput {
                offset: 0,
                project_id: project.id.clone(),
                query: "语音打断".to_string(),
                source_types: vec!["task".to_string()],
                task_id: None,
                limit: 20,
            })
            .unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].source_type, "task");
        assert_eq!(tasks[0].task_id.as_deref(), Some(task.id.as_str()));
        assert!(tasks[0].source_uri.contains("/tasks/"));
        assert!(tasks[0].content.contains("实时语音打断修复"));

        let conversations = fixture
            .store()
            .search_knowledge(SearchKnowledgeInput {
                offset: 0,
                project_id: project.id,
                query: "完整朗读".to_string(),
                source_types: vec!["conversation".to_string()],
                task_id: None,
                limit: 20,
            })
            .unwrap();
        assert_eq!(conversations.len(), 1);
        assert!(conversations[0].snippet.contains("完整朗读"));
        assert!(conversations[0].source_uri.contains("/messages/"));
    }

    #[test]
    fn knowledge_pagination_is_stable_and_filters_before_offset() {
        let fixture = TestStore::new();
        let project = fixture.project();
        for index in 0..5 {
            fixture.store().record_message(&project.id, "coordinator", &format!("paginationtoken record {index}")).unwrap();
        }
        for query in ["", "paginationtoken"] {
            let search = |offset, limit| fixture.store().search_knowledge(SearchKnowledgeInput {
                project_id: project.id.clone(), query: query.into(), source_types: vec!["conversation".into()], task_id: None, offset, limit,
            }).unwrap();
            let all = search(0, 20);
            assert_eq!(all.len(), 5);
            let mut pages = search(0, 2);
            pages.extend(search(2, 2));
            pages.extend(search(4, 2));
            assert_eq!(pages.iter().map(|row| &row.id).collect::<Vec<_>>(), all.iter().map(|row| &row.id).collect::<Vec<_>>());
            assert!(search(5, 2).is_empty());
        }
        let legacy: SearchKnowledgeInput = serde_json::from_value(serde_json::json!({"projectId": project.id, "query": "", "taskId": null, "limit": 20})).unwrap();
        assert_eq!(legacy.offset, 0);
    }

    #[test]
    fn knowledge_index_removes_sources_deleted_from_the_project() {
        let fixture = TestStore::new();
        let project = fixture.project();
        fixture
            .store()
            .record_message(&project.id, "coordinator", "这是一条即将删除的知识记录")
            .unwrap();
        assert_eq!(
            fixture
                .store()
                .search_knowledge(SearchKnowledgeInput {
                offset: 0,
                    project_id: project.id.clone(),
                    query: "即将删除".to_string(),
                    source_types: vec!["conversation".to_string()],
                    task_id: None,
                    limit: 20,
                })
                .unwrap()
                .len(),
            1
        );

        fixture
            .store()
            .clear_coordinator_messages(&project.id)
            .unwrap();
        assert!(fixture
            .store()
            .search_knowledge(SearchKnowledgeInput {
                offset: 0,
                project_id: project.id,
                query: "即将删除".to_string(),
                source_types: vec!["conversation".to_string()],
                task_id: None,
                limit: 20,
            })
            .unwrap()
            .is_empty());
    }

    #[test]
    fn unverified_memory_is_promoted_only_after_successful_delivery() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "沉淀经验");
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .put_memory(PutMemoryInput {
                project_id: project.id.clone(),
                namespace: "pattern".to_string(),
                key: "safe-edit".to_string(),
                value: "先验证再提交".to_string(),
                source_task_id: Some(task.id.clone()),
                source_run_id: Some(run.id.clone()),
                confidence: 0.8,
            })
            .unwrap();
        assert!(fixture
            .store()
            .search_memory(SearchMemoryInput {
                project_id: project.id.clone(),
                query: "safe-edit".to_string(),
                namespace: None,
                current_run_id: None,
                limit: 10,
            })
            .unwrap()
            .is_empty());
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store()
            .finalize_successful_run(&run.id, "workspace", "完成", true, Some("hash"), None)
            .unwrap();

        let memories = fixture
            .store()
            .search_memory(SearchMemoryInput {
                project_id: project.id,
                query: "safe-edit".to_string(),
                namespace: None,
                current_run_id: None,
                limit: 10,
            })
            .unwrap();
        assert_eq!(memories.len(), 1);
        assert!(memories[0].verified);
    }

    #[test]
    fn capability_router_reassigns_an_incompatible_agent() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let claude = fixture
            .store()
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: project.id.clone(),
                name: "Claude".to_string(),
                runtime_kind: AgentRuntimeKind::ClaudeCode,
                role: AgentRole::Worker,
                description: "实现".to_string(),
                model: None,
                permission_profile: PermissionProfile::Auto,
                max_concurrency: 1,
            persona: Default::default(),
            persona_custom: String::new(),
            })
            .unwrap();
        let codex = fixture.agent(&project.id, "Codex");
        let task = fixture
            .store()
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "需要差异能力".to_string(),
                description: String::new(),
                acceptance_criteria: "完成".to_string(),
                verification_command: None,
                assignee_agent_id: Some(claude.id),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::Worktree,
                priority: 0,
                required_capabilities: vec!["diff".to_string()],
                max_attempts: 3,
                retry_backoff_seconds: 5,
            })
            .unwrap();

        assert_eq!(
            fixture.store().route_runnable_tasks(&project.id).unwrap(),
            1
        );
        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        let routed = snapshot
            .tasks
            .iter()
            .find(|item| item.id == task.id)
            .unwrap();
        assert_eq!(routed.assignee_agent_id.as_deref(), Some(codex.id.as_str()));
        assert!(snapshot.events.iter().any(|event| {
            event.entity_id == task.id && event.event_type == "task.capability_routed"
        }));
    }

    #[test]
    fn agent_requested_approval_pauses_and_resumes_the_active_run() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "Codex");
        let task = fixture.task(&project.id, &agent.id, "高风险修改");
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store()
            .update_run(UpdateRunInput {
                run_id: run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        let approval = fixture
            .store()
            .request_approval(RequestApprovalInput {
                project_id: project.id.clone(),
                task_id: Some(task.id.clone()),
                run_id: Some(run.id.clone()),
                approval_type: "destructive_change".to_string(),
                summary: "需要修改生成文件".to_string(),
            })
            .unwrap();
        let waiting = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(
            waiting
                .tasks
                .iter()
                .find(|item| item.id == task.id)
                .unwrap()
                .status,
            TaskStatus::WaitingApproval
        );
        assert_eq!(
            waiting
                .runs
                .iter()
                .find(|item| item.id == run.id)
                .unwrap()
                .status,
            RunStatus::WaitingApproval
        );
        assert_eq!(approval.status, "pending");

        fixture.store().approve_task(&task.id).unwrap();
        let resumed = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(
            resumed
                .tasks
                .iter()
                .find(|item| item.id == task.id)
                .unwrap()
                .status,
            TaskStatus::Running
        );
        assert_eq!(
            resumed
                .runs
                .iter()
                .find(|item| item.id == run.id)
                .unwrap()
                .status,
            RunStatus::Running
        );
        assert_eq!(
            resumed
                .approvals
                .iter()
                .find(|item| item.id == approval.id)
                .unwrap()
                .status,
            "approved"
        );
    }

    #[test]
    fn deepseek_runtime_approval_cancel_does_not_grant_permission() {
        let fixture = TestStore::new();
        let project = fixture.project();
        let agent = fixture.agent(&project.id, "DeepSeek");
        let task = fixture.task(&project.id, &agent.id, "权限检查");
        let run = fixture
            .store()
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        let approval = fixture
            .store()
            .request_approval(RequestApprovalInput {
                project_id: project.id.clone(),
                task_id: Some(task.id.clone()),
                run_id: Some(run.id),
                approval_type: "runtime_permission".into(),
                summary: "测试权限".into(),
            })
            .unwrap();
        fixture
            .store()
            .cancel_runtime_approval(&approval.id)
            .unwrap();
        fixture.store().approve_task(&task.id).unwrap();
        let snapshot = fixture.store().snapshot(&project.id).unwrap();
        assert_eq!(
            snapshot
                .approvals
                .iter()
                .find(|item| item.id == approval.id)
                .unwrap()
                .status,
            "cancelled"
        );
    }
}
