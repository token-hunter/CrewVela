use super::*;
use serde::{Deserialize, Serialize};

const DESIGN_TASK_PREFIX: &str = "delegation-design-";

pub(in crate::multi_agent) fn is_design_task(id: &str) -> bool {
    id.starts_with(DESIGN_TASK_PREFIX)
}

pub(super) fn migrate(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS ma_design_delegations (
            id TEXT PRIMARY KEY,
            project_id TEXT NOT NULL REFERENCES ma_projects(id) ON DELETE CASCADE,
            conversation_id TEXT NOT NULL,
            source_task_id TEXT NOT NULL REFERENCES ma_tasks(id) ON DELETE CASCADE,
            source_run_id TEXT NOT NULL UNIQUE REFERENCES ma_runs(id) ON DELETE CASCADE,
            source_agent_id TEXT NOT NULL,
            target_agent_id TEXT NOT NULL,
            title TEXT NOT NULL,
            brief TEXT NOT NULL,
            criteria TEXT NOT NULL,
            checkpoint_note TEXT NOT NULL,
            requested_at INTEGER NOT NULL,
            design_task_id TEXT REFERENCES ma_tasks(id) ON DELETE CASCADE,
            checkpoint_at INTEGER,
            checkpoint_json TEXT,
            delivery_run_id TEXT REFERENCES ma_runs(id) ON DELETE SET NULL,
            delivered_at INTEGER,
            delivery_json TEXT,
            accepted_at INTEGER,
            resume_run_id TEXT REFERENCES ma_runs(id) ON DELETE SET NULL,
            resumed_at INTEGER
        );
        CREATE INDEX IF NOT EXISTS ma_design_delegations_thread
            ON ma_design_delegations(project_id,conversation_id);
        CREATE UNIQUE INDEX IF NOT EXISTS ma_design_delegations_child
            ON ma_design_delegations(design_task_id);",
    )?;
    Ok(())
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(in crate::multi_agent) struct RequestDesign {
    target_agent_id: String,
    title: String,
    brief: String,
    criteria: Vec<String>,
    checkpoint: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DesignDelivery {
    filename: String,
    markdown: String,
    acceptance_evidence: Vec<DesignCriterion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DesignCriterion {
    criterion: String,
    evidence: String,
    met: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesignDelegation {
    id: String,
    source_agent_id: String,
    target_agent_id: String,
    source_run_id: String,
    delivery_run_id: Option<String>,
    resume_run_id: Option<String>,
    title: String,
    brief: String,
    checkpoint_at: Option<i64>,
    submitted_at: Option<i64>,
    accepted_at: Option<i64>,
    resumed_at: Option<i64>,
    filename: Option<String>,
    status: &'static str,
    failure: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(in crate::multi_agent) struct DesignCheckpoint {
    pub cwd: String,
    pub head: String,
    runtime: AgentRuntimeKind,
    permission: PermissionProfile,
    role: AgentRole,
    write_scope: String,
}

fn bounded(value: &str, name: &str, max: usize) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > max || value.contains('\0') {
        anyhow::bail!("{name}为空或超过长度限制");
    }
    Ok(value.to_owned())
}

fn active_run(connection: &Connection, run_id: &str, agent_id: &str) -> Result<AgentRun> {
    let run = get_run_with(connection, run_id)?;
    if run.agent_id != agent_id
        || run.status != RunStatus::Running
        || run
            .session_locator
            .as_ref()
            .is_some_and(|value| value["stopUnverified"] == true)
    {
        anyhow::bail!("当前运行不可办理设计委派");
    }
    ensure_no_handoff(connection, &run.project_id)?;
    let manual = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM ma_agent_sessions WHERE current_run_id=?1 AND control_mode='manual')",
        [run_id], |row| row.get::<_, bool>(0),
    )?;
    if manual {
        anyhow::bail!("当前会话已由用户接管，不能自动办理委派");
    }
    Ok(run)
}

pub(super) fn charged_attempt(connection: &Connection, task_id: &str, attempt: u32) -> Result<u32> {
    if !task_id.starts_with("conversation-task-") {
        return Ok(attempt);
    }
    let checkpoints: u32 = connection.query_row(
        "SELECT COUNT(*) FROM ma_design_delegations WHERE source_task_id=?1 AND checkpoint_at IS NOT NULL",
        [task_id], |row| row.get(0),
    )?;
    Ok(attempt.saturating_sub(checkpoints).max(1))
}

fn approved_scope(connection: &Connection, task_id: &str) -> Result<(String, String)> {
    connection
        .query_row(
            "SELECT json_extract(r.data_json,'$.conversationId'),s.write_paths_json
         FROM ma_conversation_task_scopes s JOIN ma_conversation_plans p ON p.id=s.plan_id
         JOIN ma_conversation_rounds r ON r.id=p.round_id
         WHERE s.task_id=?1 AND p.applied_request_id IS NOT NULL",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .context("设计委派需要已确认的团队分工任务")
}

fn approved_participants(connection: &Connection, task_id: &str) -> Result<Vec<String>> {
    let participants: String = connection
        .query_row(
            "SELECT json_extract(r.data_json,'$.participantAgentIds')
         FROM ma_conversation_task_scopes s JOIN ma_conversation_plans p ON p.id=s.plan_id
         JOIN ma_conversation_rounds r ON r.id=p.round_id
         WHERE s.task_id=?1 AND p.applied_request_id IS NOT NULL",
            [task_id],
            |row| row.get(0),
        )
        .context("无法读取原分工的已授权成员")?;
    Ok(serde_json::from_str(&participants)?)
}

fn can_receive_design(agent: &AgentProfile) -> bool {
    agent.enabled
        && matches!(
            agent.runtime_kind,
            AgentRuntimeKind::ClaudeCode | AgentRuntimeKind::Codex
        )
}

impl MultiAgentStore {
    pub(in crate::multi_agent) fn delegation_tools_for_run(
        &self,
        task_id: &str,
    ) -> Result<Option<&'static str>> {
        if is_design_task(task_id) {
            return Ok(Some("deliver"));
        }
        if !task_id.starts_with("conversation-task-") {
            return Ok(None);
        }
        let connection = self.connection.lock().unwrap();
        let task = get_task_with(&connection, task_id)?;
        if task.workspace_mode != WorkspaceMode::Worktree {
            return Ok(None);
        }
        let can_delegate = task
            .assignee_agent_id
            .as_deref()
            .map(|id| get_agent_with(&connection, id))
            .transpose()?
            .is_some_and(|agent| agent.role != AgentRole::Reviewer);
        Ok(can_delegate.then_some("request"))
    }

    pub(in crate::multi_agent) fn design_recipients(
        &self,
        task_id: &str,
        agent_id: &str,
    ) -> Result<Vec<serde_json::Value>> {
        if self.delegation_tools_for_run(task_id)? != Some("request") {
            return Ok(Vec::new());
        }
        let connection = self.connection.lock().unwrap();
        let task = get_task_with(&connection, task_id)?;
        let participants = approved_participants(&connection, task_id)?;
        let disabled = crate::runtime_preferences::get()?.disabled_runtimes;
        Ok(list_agents_with(&connection, &task.project_id)?.into_iter()
            .filter(|agent| agent.id != agent_id && participants.contains(&agent.id)
                && can_receive_design(agent) && !disabled.contains(&agent.runtime_kind))
            .map(|agent| serde_json::json!({"id": agent.id, "name": agent.name, "role": agent.role, "description": agent.description}))
            .collect())
    }

    pub(in crate::multi_agent) fn request_design(
        &self,
        run_id: &str,
        agent_id: &str,
        mut input: RequestDesign,
    ) -> Result<String> {
        input.title = bounded(&input.title, "委派标题", 240)?;
        input.brief = bounded(&input.brief, "设计说明", 8_000)?;
        input.checkpoint = bounded(&input.checkpoint, "检查点说明", 4_000)?;
        if input.criteria.is_empty() || input.criteria.len() > 8 {
            anyhow::bail!("设计委派需要 1 至 8 项明确的验收标准");
        }
        input.criteria = input
            .criteria
            .iter()
            .map(|value| bounded(value, "验收标准", 480))
            .collect::<Result<_>>()?;
        if input
            .criteria
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != input.criteria.len()
        {
            anyhow::bail!("验收标准不得重复");
        }
        let criteria_json = serde_json::to_string(&input.criteria)?;
        let target = self.agent_profile(&input.target_agent_id)?;
        let _runtime = crate::runtime_preferences::acquire(target.runtime_kind)?;
        let connection = &mut *self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = active_run(&tx, run_id, agent_id)?;
        let source = get_task_with(&tx, &run.task_id)?;
        let target = get_agent_with(&tx, &input.target_agent_id)?;
        let source_agent = get_agent_with(&tx, agent_id)?;
        let project = self.get_project_with(&tx, &run.project_id)?;
        if target.project_id != run.project_id
            || target.id == agent_id
            || !can_receive_design(&target)
            || source.workspace_mode != WorkspaceMode::Worktree
            || source_agent.role == AgentRole::Reviewer
            || !source_agent.enabled
            || source.status != TaskStatus::Running
            || project.status != ProjectStatus::Active
        {
            anyhow::bail!("仅可从正在执行的独立工作树委派给本项目中支持只读设计的另一位成员");
        }
        let (conversation, _) = approved_scope(&tx, &source.id)?;
        if !approved_participants(&tx, &source.id)?.contains(&target.id) {
            anyhow::bail!("设计接收者不在原分工明确选择的成员中");
        }
        let previous = tx
            .query_row(
                "SELECT id,target_agent_id,title,brief,criteria,checkpoint_note
             FROM ma_design_delegations WHERE source_run_id=?1",
                [run_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                    ))
                },
            )
            .optional()?;
        if let Some((id, earlier_target, title, brief, criteria, checkpoint)) = previous {
            if input.target_agent_id == earlier_target
                && input.title == title
                && input.brief == brief
                && criteria_json == criteria
                && input.checkpoint == checkpoint
            {
                return Ok(id);
            }
            anyhow::bail!("当前执行已登记一项设计委派");
        }
        if tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_design_delegations WHERE source_task_id=?1 AND target_agent_id=?2 AND title=?3 AND brief=?4 AND accepted_at IS NOT NULL)",
            params![source.id, target.id, input.title, input.brief], |row| row.get::<_, bool>(0),
        )? {
            anyhow::bail!("这项设计已交付，请使用原交付继续执行");
        }
        let id = uuid::Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO ma_design_delegations
             (id,project_id,conversation_id,source_task_id,source_run_id,source_agent_id,target_agent_id,title,brief,criteria,checkpoint_note,requested_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            params![id,run.project_id,conversation,source.id,run.id,agent_id,target.id,input.title,
                input.brief,criteria_json,input.checkpoint,now_millis()],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub(in crate::multi_agent) fn design_requested(&self, run_id: &str) -> Result<bool> {
        let connection = self.connection.lock().unwrap();
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_design_delegations WHERE source_run_id=?1 AND design_task_id IS NULL)",
            [run_id], |row| row.get(0),
        )?)
    }

    pub(in crate::multi_agent) fn checkpoint_design_request(
        &self,
        run_id: &str,
        agent: &AgentProfile,
        cwd: &str,
        head: &str,
        locator: &serde_json::Value,
    ) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = active_run(&tx, run_id, &agent.id)?;
        let task = get_task_with(&tx, &run.task_id)?;
        if self.get_project_with(&tx, &run.project_id)?.status != ProjectStatus::Active {
            anyhow::bail!("项目已暂停，不能派发新设计任务");
        }
        let (_, write_scope) = approved_scope(&tx, &task.id)?;
        let (id, target, title, brief, criteria, note): (
            String,
            String,
            String,
            String,
            String,
            String,
        ) = tx.query_row(
            "SELECT id,target_agent_id,title,brief,criteria,checkpoint_note
             FROM ma_design_delegations WHERE source_run_id=?1 AND design_task_id IS NULL",
            [run_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )?;
        let target_profile = get_agent_with(&tx, &target)?;
        if !can_receive_design(&target_profile) || target_profile.project_id != run.project_id {
            anyhow::bail!("设计接收者已不可用");
        }
        let current_agent = get_agent_with(&tx, &agent.id)?;
        if !current_agent.enabled
            || current_agent.runtime_kind != agent.runtime_kind
            || current_agent.permission_profile != agent.permission_profile
            || current_agent.role != agent.role
            || task.workspace_mode != WorkspaceMode::Worktree
            || task.assignee_agent_id.as_deref() != Some(&agent.id)
        {
            anyhow::bail!("原任务的执行授权已变更");
        }
        let child = format!("{DESIGN_TASK_PREFIX}{id}");
        let stamp = now_millis();
        let description = format!("设计需求\n{brief}\n\n原任务检查点摘要\n{note}");
        let criteria = serde_json::from_str::<Vec<String>>(&criteria)?.join("\n");
        tx.execute(
            "INSERT INTO ma_tasks (id,project_id,parent_id,stage,title,description,acceptance_criteria,
             status,assignee_agent_id,workspace_mode,priority,created_at,updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,'ready',?8,'read_only',?9,?10,?10)",
            params![child,run.project_id,task.id,task.stage,title,description,criteria,target,task.priority,stamp],
        )?;
        tx.execute(
            "INSERT INTO ma_conversation_task_scopes(task_id,plan_id,write_paths_json)
             SELECT ?2,plan_id,'[]' FROM ma_conversation_task_scopes WHERE task_id=?1",
            params![task.id, child],
        )?;
        // The child has no dependency on its parent and receives no writable scope.
        tx.execute(
            "INSERT INTO ma_task_dependencies(task_id,depends_on_task_id) VALUES (?1,?2)",
            params![task.id, child],
        )?;
        let checkpoint = DesignCheckpoint {
            cwd: cwd.to_owned(),
            head: head.to_owned(),
            runtime: agent.runtime_kind,
            permission: agent.permission_profile,
            role: agent.role,
            write_scope,
        };
        tx.execute("UPDATE ma_design_delegations SET design_task_id=?2,checkpoint_at=?3,checkpoint_json=?4 WHERE id=?1",
            params![id,child,stamp,serde_json::to_string(&checkpoint)?])?;
        tx.execute(
            "UPDATE ma_tasks SET status='blocked',next_retry_at=NULL,updated_at=?2 WHERE id=?1",
            params![task.id, stamp],
        )?;
        let mut stable_locator = locator.clone();
        stable_locator["cwd"] = serde_json::Value::String(cwd.to_owned());
        tx.execute("UPDATE ma_runs SET status='interrupted',session_locator_json=?2,output_summary=?3,finished_at=?4 WHERE id=?1",
            params![run.id,stable_locator.to_string(),brief,stamp])?;
        tx.execute(
            "UPDATE ma_agent_sessions SET status='completed',updated_at=?2 WHERE current_run_id=?1",
            params![run.id, stamp],
        )?;
        append_event(
            &tx,
            &run.project_id,
            "task",
            &task.id,
            "delegation.checkpoint_saved",
            "agent",
            Some(&agent.id),
            serde_json::json!({"delegationId":id,"designTaskId":child,"head":head}),
            stamp,
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(in crate::multi_agent) fn submit_design(
        &self,
        run_id: &str,
        agent_id: &str,
        mut input: DesignDelivery,
    ) -> Result<()> {
        input.filename = bounded(&input.filename, "文件名", 170)?;
        if !input.filename.ends_with(".md")
            || input.filename == ".md"
            || input.filename.contains(['\\', '/', ':'])
            || input.filename.chars().any(char::is_control)
        {
            anyhow::bail!("交付物名称必须是不含路径的 Markdown 文件名");
        }
        input.markdown = bounded(&input.markdown, "设计交付正文", 32_000)?;
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = active_run(&tx, run_id, agent_id)?;
        let (id, criteria, previous_run, previous_json): (
            String,
            String,
            Option<String>,
            Option<String>,
        ) = tx
            .query_row(
                "SELECT id,criteria,delivery_run_id,delivery_json FROM ma_design_delegations
                 WHERE design_task_id=?1 AND target_agent_id=?2 AND accepted_at IS NULL",
                params![run.task_id, agent_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .context("当前运行不是这项设计委派的接收方")?;
        let criteria: Vec<String> = serde_json::from_str(&criteria)?;
        if input.acceptance_evidence.len() != criteria.len() {
            anyhow::bail!("交付物必须逐项说明全部验收标准");
        }
        for criterion in criteria {
            let evidence = input
                .acceptance_evidence
                .iter_mut()
                .find(|item| item.criterion == criterion)
                .context("交付物缺少与原文匹配的验收标准")?;
            if !evidence.met {
                anyhow::bail!("交付物仍有未满足的验收标准：{criterion}");
            }
            evidence.evidence = bounded(&evidence.evidence, "验收依据", 1_500)?;
        }
        let delivery = serde_json::to_string(&input)?;
        if previous_run.as_deref() == Some(run_id) && previous_json.as_deref() != Some(&delivery) {
            anyhow::bail!("本次运行已提交交付，请勿重复提交不同版本");
        }
        tx.execute("UPDATE ma_design_delegations SET delivery_run_id=?2,delivery_json=?3,delivered_at=?4 WHERE id=?1",
            params![id,run_id,delivery,now_millis()])?;
        tx.commit()?;
        Ok(())
    }

    pub(in crate::multi_agent) fn settle_design(
        &self,
        run_id: &str,
        agent_id: &str,
        summary: &str,
        locator: &serde_json::Value,
    ) -> Result<()> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = active_run(&tx, run_id, agent_id)?;
        let (id, parent, evidence): (String, String, String) = tx.query_row(
            "SELECT id,source_task_id,delivery_json FROM ma_design_delegations
             WHERE design_task_id=?1 AND target_agent_id=?2 AND delivery_run_id=?3 AND accepted_at IS NULL",
            params![run.task_id,agent_id,run_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
        ).context("需要使用 cc_design_submit 提交交付正文和对照验收标准的依据")?;
        let delivery: DesignDelivery = serde_json::from_str(&evidence)?;
        let summary: String = summary.trim().chars().take(1_000).collect();
        let stamp = now_millis();
        tx.execute(
            "UPDATE ma_design_delegations SET accepted_at=?2 WHERE id=?1",
            params![id, stamp],
        )?;
        tx.execute("UPDATE ma_runs SET status='succeeded',session_locator_json=?2,output_summary=?3,finished_at=?4 WHERE id=?1",
            params![run.id,locator.to_string(),summary,stamp])?;
        tx.execute(
            "UPDATE ma_agent_sessions SET status='completed',updated_at=?2 WHERE current_run_id=?1",
            params![run.id, stamp],
        )?;
        tx.execute(
            "UPDATE ma_tasks SET status='done',updated_at=?2 WHERE id=?1",
            params![run.task_id, stamp],
        )?;
        tx.execute(
            "INSERT INTO ma_artifacts (id,project_id,task_id,run_id,artifact_type,uri,summary,created_at)
             VALUES (?1,?2,?3,?4,'design_document',?5,?6,?7)",
            params![uuid::Uuid::new_v4().to_string(),run.project_id,run.task_id,run.id,
                format!("crewvela://design-delivery/{id}"),delivery.filename,stamp],
        )?;
        unblock_ready_tasks(&tx, &run.project_id, stamp)?;
        append_event(
            &tx,
            &run.project_id,
            "task",
            &parent,
            "delegation.delivery_accepted",
            "system",
            None,
            serde_json::json!({"delegationId":id,"designTaskId":run.task_id,"filename":delivery.filename}),
            stamp,
        )?;
        tx.commit()?;
        Ok(())
    }

    pub(in crate::multi_agent) fn pending_design_checkpoint(
        &self,
        task: &MultiAgentTask,
        agent: &AgentProfile,
    ) -> Result<Option<DesignCheckpoint>> {
        let connection = self.connection.lock().unwrap();
        let checkpoint = connection.query_row(
            "SELECT checkpoint_json FROM ma_design_delegations WHERE source_task_id=?1 AND accepted_at IS NOT NULL AND resumed_at IS NULL",
            [&task.id], |row| row.get::<_, String>(0),
        ).optional()?;
        let Some(raw) = checkpoint else {
            return Ok(None);
        };
        let checkpoint: DesignCheckpoint = serde_json::from_str(&raw)?;
        let (_, write_scope) = approved_scope(&connection, &task.id)?;
        if task.workspace_mode != WorkspaceMode::Worktree
            || task.assignee_agent_id.as_deref() != Some(&agent.id)
            || agent.runtime_kind != checkpoint.runtime
            || agent.permission_profile != checkpoint.permission
            || agent.role != checkpoint.role
            || write_scope != checkpoint.write_scope
        {
            anyhow::bail!("原任务的执行授权已变更，不能自动续跑");
        }
        Ok(Some(checkpoint))
    }

    pub(in crate::multi_agent) fn mark_design_resumed(
        &self,
        run_id: &str,
        task_id: &str,
    ) -> Result<()> {
        let connection = self.connection.lock().unwrap();
        connection.execute(
            "UPDATE ma_design_delegations SET resume_run_id=?2,resumed_at=?3
             WHERE source_task_id=?1 AND accepted_at IS NOT NULL AND resumed_at IS NULL
             AND EXISTS(SELECT 1 FROM ma_runs WHERE id=?2 AND task_id=?1 AND status='running')",
            params![task_id, run_id, now_millis()],
        )?;
        Ok(())
    }

    pub(in crate::multi_agent) fn design_continuation_context(
        &self,
        task_id: &str,
    ) -> Result<String> {
        let connection = self.connection.lock().unwrap();
        let mut query = connection.prepare(
            "SELECT title,criteria,checkpoint_note,delivery_json FROM ma_design_delegations
             WHERE source_task_id=?1 AND accepted_at IS NOT NULL ORDER BY accepted_at DESC LIMIT 4",
        )?;
        let accepted = query
            .query_map([task_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        accepted.into_iter().map(|(title,criteria,note,content)| {
            let delivery: DesignDelivery = serde_json::from_str(&content)?;
            let evidence = delivery.acceptance_evidence.iter().map(|item| format!("- {}：{}", item.criterion, item.evidence))
                .collect::<Vec<_>>().join("\n");
            Ok(format!("委派 · {title}\n原检查点：{note}\n验收标准：{criteria}\n交付：{}\n{}\n验收依据：\n{evidence}",
                delivery.filename,delivery.markdown))
        }).collect::<Result<Vec<_>>>().map(|items| items.join("\n\n"))
    }

    pub(in crate::multi_agent) fn design_source_head(
        &self,
        design_task_id: &str,
    ) -> Result<String> {
        let connection = self.connection.lock().unwrap();
        connection.query_row(
            "SELECT json_extract(checkpoint_json,'$.head') FROM ma_design_delegations WHERE design_task_id=?1",
            [design_task_id], |row| row.get(0),
        ).context("找不到设计任务的原工作树检查点")
    }

    pub(crate) fn design_delivery(
        &self,
        project: &str,
        conversation: &str,
        id: &str,
    ) -> Result<DesignDelivery> {
        let connection = self.connection.lock().unwrap();
        let raw: String = connection
            .query_row(
                "SELECT delivery_json FROM ma_design_delegations
             WHERE id=?1 AND project_id=?2 AND conversation_id=?3 AND accepted_at IS NOT NULL",
                params![id, project, conversation],
                |row| row.get(0),
            )
            .context("该会话中暂无已验收的交付物")?;
        Ok(serde_json::from_str(&raw)?)
    }
}

pub(in crate::multi_agent) fn conversation_designs(
    connection: &Connection,
    project: &str,
    conversation: &str,
) -> Result<Vec<DesignDelegation>> {
    let mut query = connection.prepare(
        "SELECT d.id,d.source_agent_id,d.target_agent_id,d.source_run_id,d.delivery_run_id,d.resume_run_id,
            d.title,d.brief,d.checkpoint_at,d.accepted_at,d.resumed_at,
            CASE WHEN d.accepted_at IS NOT NULL THEN json_extract(d.delivery_json,'$.filename') END,
            source.status, design.status,
            CASE WHEN d.accepted_at IS NULL THEN COALESCE(
                (SELECT error FROM ma_runs WHERE task_id=d.design_task_id ORDER BY attempt DESC LIMIT 1), source.error)
                WHEN d.resumed_at IS NULL AND parent.status IN ('failed','cancelled') THEN
                (SELECT error FROM ma_runs WHERE task_id=d.source_task_id ORDER BY attempt DESC LIMIT 1) END,
            d.delivered_at, parent.status
         FROM ma_design_delegations d JOIN ma_runs source ON source.id=d.source_run_id
         JOIN ma_tasks parent ON parent.id=d.source_task_id
         LEFT JOIN ma_tasks design ON design.id=d.design_task_id
         WHERE d.project_id=?1 AND d.conversation_id=?2 ORDER BY d.requested_at,d.id",
    )?;
    let rows = query.query_map(params![project, conversation], |row| {
        let checkpoint_at: Option<i64> = row.get(8)?;
        let accepted_at: Option<i64> = row.get(9)?;
        let resumed_at: Option<i64> = row.get(10)?;
        let source: String = row.get(12)?;
        let design: Option<String> = row.get(13)?;
        let parent: String = row.get(16)?;
        let status = if resumed_at.is_some() {
            "resumed"
        } else if accepted_at.is_some() && matches!(parent.as_str(), "failed" | "cancelled") {
            "failed"
        } else if accepted_at.is_some() {
            "accepted"
        } else if matches!(design.as_deref(), Some("failed" | "cancelled"))
            || (checkpoint_at.is_none()
                && !matches!(
                    source.as_str(),
                    "starting" | "running" | "waiting_approval" | "waiting_input"
                ))
        {
            "failed"
        } else if checkpoint_at.is_some() {
            "waiting_design"
        } else {
            "checkpointing"
        };
        Ok(DesignDelegation {
            id: row.get(0)?,
            source_agent_id: row.get(1)?,
            target_agent_id: row.get(2)?,
            source_run_id: row.get(3)?,
            delivery_run_id: if accepted_at.is_some() {
                row.get(4)?
            } else {
                None
            },
            resume_run_id: row.get(5)?,
            title: row.get(6)?,
            brief: row.get(7)?,
            checkpoint_at,
            submitted_at: row.get(15)?,
            accepted_at,
            resumed_at,
            filename: row.get(11)?,
            status,
            failure: row.get(14)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
