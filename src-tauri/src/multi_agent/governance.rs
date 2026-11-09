use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};

use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension, Row, TransactionBehavior};

use super::{
    InterventionAction, InterventionKind, InterventionRecommendedAction, InterventionRecord,
    InterventionResolutionReceipt, InterventionSeverity, InterventionStatus, MultiAgentSnapshot,
    MultiAgentStore, ResolveInterventionInput, SendCollaborationMessageInput, TaskStatus,
    WatchdogDisposition,
};

#[derive(Clone)]
pub struct CollaborationGovernance {
    store: MultiAgentStore,
}

#[derive(Debug)]
struct Candidate {
    project_id: String,
    task_id: Option<String>,
    run_id: Option<String>,
    kind: InterventionKind,
    severity: InterventionSeverity,
    title: String,
    summary: String,
    action: InterventionRecommendedAction,
    fingerprint: String,
    metadata: serde_json::Value,
}

impl CollaborationGovernance {
    pub fn new(store: MultiAgentStore) -> Self {
        Self { store }
    }

    pub fn list_interventions(&self) -> Result<Vec<InterventionRecord>> {
        self.synchronize_interventions()?;
        let connection = self.store.connection.lock().unwrap();
        let mut statement = connection.prepare(
            "SELECT i.id, i.project_id, p.name, i.task_id, i.run_id, i.kind, i.severity,
                    i.title, i.summary, i.status, i.recommended_action, i.fingerprint,
                    i.snoozed_until, i.metadata_json, i.created_at, i.updated_at, i.resolved_at
             FROM ma_interventions i
             JOIN ma_projects p ON p.id = i.project_id
             WHERE i.status = 'open'
             ORDER BY CASE i.severity
                        WHEN 'critical' THEN 0 WHEN 'high' THEN 1
                        WHEN 'medium' THEN 2 ELSE 3 END,
                      i.updated_at DESC",
        )?;
        let items = statement
            .query_map([], intervention_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(items)
    }

    pub fn resolve(
        &self,
        input: ResolveInterventionInput,
    ) -> Result<InterventionResolutionReceipt> {
        self.synchronize_interventions()?;
        let intervention = {
            let connection = self.store.connection.lock().unwrap();
            get_intervention(&connection, &input.intervention_id)?
        };
        if intervention.status == InterventionStatus::Resolved {
            return Ok(InterventionResolutionReceipt {
                intervention_id: intervention.id,
                project_id: intervention.project_id,
                resume_project: false,
                message: "该干预事项已经处理".to_string(),
            });
        }

        let mut resume_project = false;
        let message = match input.action {
            InterventionAction::Approve => {
                let task_id = intervention
                    .task_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("该干预事项没有可批准的任务"))?;
                let snapshot = self.store.snapshot(&intervention.project_id)?;
                let task = snapshot.tasks.iter().find(|task| task.id == task_id).ok_or_else(|| anyhow!("待批准任务不存在"))?;
                let delivery_request = intervention.metadata["approvalId"].as_str().is_some_and(|id| snapshot.approvals.iter().any(|approval| approval.id == id && approval.status == "pending" && approval.approval_type == "task_review"));
                if task.status == TaskStatus::InReview && delivery_request {
                    if let Err(error) = super::merge_integrator_delivery(&self.store, task_id) {
                        let message = error.to_string();
                        let _ = self.store.request_task_changes(task_id, &message);
                        return Err(anyhow!("{message}；任务已转为需要修改"));
                    }
                    self.store.approve_task(task_id)?;
                } else {
                    self.approve_runtime_request(&intervention)?;
                }
                resume_project = true;
                "已批准并继续项目".to_string()
            }
            InterventionAction::Retry => {
                let task_id = intervention
                    .task_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("该干预事项没有可重试的任务"))?;
                self.store.retry_task(task_id)?;
                resume_project = true;
                "已重新排队并继续项目".to_string()
            }
            InterventionAction::Reassign => {
                let task_id = intervention
                    .task_id
                    .as_deref()
                    .ok_or_else(|| anyhow!("该干预事项没有可重派的任务"))?;
                let agent_id = input
                    .agent_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| anyhow!("重派任务时必须选择 Agent"))?;
                self.store.reassign_task(task_id, agent_id)?;
                resume_project = true;
                "已重派任务并继续项目".to_string()
            }
            InterventionAction::Respond => {
                let response = input
                    .response
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| anyhow!("回复内容不能为空"))?;
                self.respond_to_waiting_run(&intervention, response)?;
                resume_project = true;
                "已回复 Agent 并继续项目".to_string()
            }
            InterventionAction::Snooze => {
                let minutes = input.snooze_minutes.unwrap_or(60).clamp(5, 10_080);
                let until = now_millis().saturating_add(i64::from(minutes) * 60_000);
                let connection = self.store.connection.lock().unwrap();
                connection.execute(
                    "UPDATE ma_interventions
                     SET status = 'snoozed', snoozed_until = ?2, updated_at = ?3
                     WHERE id = ?1 AND status != 'resolved'",
                    params![intervention.id, until, now_millis()],
                )?;
                return Ok(InterventionResolutionReceipt {
                    intervention_id: intervention.id,
                    project_id: intervention.project_id,
                    resume_project: false,
                    message: format!("已暂缓 {minutes} 分钟"),
                });
            }
            InterventionAction::Dismiss => "已忽略本次干预事项".to_string(),
        };

        let now = now_millis();
        let connection = self.store.connection.lock().unwrap();
        connection.execute(
            "UPDATE ma_interventions
             SET status = 'resolved', resolved_at = ?2, snoozed_until = NULL, updated_at = ?2
             WHERE id = ?1",
            params![intervention.id, now],
        )?;
        append_event(
            &connection,
            &intervention.project_id,
            "intervention",
            &intervention.id,
            "intervention.resolved",
            serde_json::json!({"action": input.action, "resumeProject": resume_project}),
            now,
        )?;
        Ok(InterventionResolutionReceipt {
            intervention_id: intervention.id,
            project_id: intervention.project_id,
            resume_project,
            message,
        })
    }

    fn approve_runtime_request(&self, intervention: &InterventionRecord) -> Result<()> {
        let approval_id = intervention.metadata.get("approvalId").and_then(serde_json::Value::as_str).ok_or_else(|| anyhow!("运行期审批缺少 approvalId"))?;
        let mut connection = self.store.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let pending: Option<(String, String)> = tx.query_row(
            "SELECT a.task_id, a.run_id FROM ma_approvals a
             JOIN ma_tasks t ON t.id=a.task_id JOIN ma_runs r ON r.id=a.run_id AND r.task_id=t.id
             WHERE a.id=?1 AND a.project_id=?2 AND a.status='pending'
               AND t.status='waiting_approval' AND r.status='waiting_approval'
               AND r.attempt=(SELECT MAX(attempt) FROM ma_runs WHERE task_id=t.id)",
            params![approval_id, intervention.project_id], |row| Ok((row.get(0)?, row.get(1)?))
        ).optional()?;
        let (task_id, run_id) = pending.ok_or_else(|| anyhow!("该运行期审批已失效或不属于当前等待审批的 Run"))?;
        if intervention.task_id.as_deref() != Some(task_id.as_str()) || intervention.run_id.as_deref() != Some(run_id.as_str()) { return Err(anyhow!("审批与干预任务/Run不匹配")); }
        let now = now_millis();
        tx.execute("UPDATE ma_approvals SET status='approved', decided_at=?2 WHERE id=?1", params![approval_id, now])?;
        let pending_count: i64 = tx.query_row("SELECT COUNT(*) FROM ma_approvals WHERE run_id=?1 AND status='pending'", params![run_id], |row| row.get(0))?;
        if pending_count == 0 {
            tx.execute("UPDATE ma_runs SET status='running', lease_expires_at=?2, last_heartbeat_at=?3 WHERE id=?1", params![run_id, now.saturating_add(120_000), now])?;
            tx.execute("UPDATE ma_tasks SET status='running', updated_at=?2 WHERE id=?1", params![task_id, now])?;
            tx.execute("UPDATE ma_agent_sessions SET status='running', updated_at=?2 WHERE current_run_id=?1", params![run_id, now])?;
        }
        append_event(&tx, &intervention.project_id, "task", &task_id, "approval.granted", serde_json::json!({"runId":run_id,"approvalId":approval_id}), now)?;
        tx.commit()?;
        Ok(())
    }

    pub fn evaluate_quiescence(&self, project_id: &str) -> Result<WatchdogDisposition> {
        let snapshot = self.store.snapshot(project_id)?;
        let watchdog_tasks = {
            let connection = self.store.connection.lock().unwrap();
            let mut statement = connection.prepare(
                "SELECT review_task_id FROM ma_semantic_watchdogs WHERE project_id = ?1",
            )?;
            let ids = statement
                .query_map(params![project_id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<HashSet<_>>>()?;
            ids
        };
        let ordinary_tasks = snapshot
            .tasks
            .iter()
            .filter(|task| !watchdog_tasks.contains(&task.id) && !is_communication_task(task))
            .collect::<Vec<_>>();
        if snapshot
            .approvals
            .iter()
            .any(|item| item.status == "pending")
            || snapshot.runs.iter().any(|run| {
                matches!(
                    run.status,
                    super::RunStatus::WaitingInput | super::RunStatus::WaitingApproval
                )
            })
        {
            self.synchronize_interventions()?;
            return Ok(WatchdogDisposition::WaitingForHuman);
        }

        if ordinary_tasks.is_empty() {
            if snapshot.tasks.iter().any(|task| {
                is_communication_task(task)
                    && matches!(task.status, TaskStatus::Failed | TaskStatus::Cancelled)
            }) {
                self.synchronize_interventions()?;
                return Ok(WatchdogDisposition::WaitingForHuman);
            }
            return Ok(WatchdogDisposition::ClearToComplete);
        }

        let fingerprint = semantic_fingerprint(&snapshot, &watchdog_tasks);
        let existing = {
            let connection = self.store.connection.lock().unwrap();
            connection
                .query_row(
                    "SELECT id, review_task_id, target_task_id, status, created_at
                     FROM ma_semantic_watchdogs WHERE fingerprint = ?1",
                    params![fingerprint],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    },
                )
                .optional()?
        };
        if let Some((watchdog_id, review_task_id, _, status, _)) = existing {
            let review_status = snapshot
                .tasks
                .iter()
                .find(|task| task.id == review_task_id)
                .map(|task| task.status);
            if status == "passed"
                && ordinary_tasks
                    .iter()
                    .all(|task| task.status == TaskStatus::Done)
            {
                return Ok(WatchdogDisposition::ClearToComplete);
            }
            if matches!(status.as_str(), "failed" | "needs_human") {
                self.synchronize_interventions()?;
                return Ok(WatchdogDisposition::WaitingForHuman);
            }
            if matches!(
                review_status,
                Some(TaskStatus::Failed | TaskStatus::Cancelled)
            ) {
                self.mark_watchdog_needs_human(
                    &watchdog_id,
                    "独立 Reviewer 未能完成语义复核，需要人工查看执行记录",
                    "failed",
                )?;
                self.synchronize_interventions()?;
                return Ok(WatchdogDisposition::WaitingForHuman);
            }
            if matches!(review_status, Some(TaskStatus::Blocked)) {
                self.mark_watchdog_needs_human(
                    &watchdog_id,
                    "语义复核任务无法进入运行队列",
                    "needs_human",
                )?;
                self.synchronize_interventions()?;
                return Ok(WatchdogDisposition::WaitingForHuman);
            }
            return Ok(WatchdogDisposition::Scheduled);
        }

        let all_done = ordinary_tasks
            .iter()
            .all(|task| task.status == TaskStatus::Done);
        let target = choose_watchdog_target(&snapshot, &watchdog_tasks)
            .ok_or_else(|| anyhow!("没有可用于语义复核的目标任务"))?;
        let all_failed = ordinary_tasks
            .iter()
            .all(|task| matches!(task.status, TaskStatus::Failed | TaskStatus::Cancelled));
        if all_failed {
            // 全部失败时语义复核没有可审的交付，复核退回目标只会再次失败；
            // 直接升级人工（失败任务经重试耗尽渠道进入干预箱），避免
            // "复核→重开→再失败"的空转。
            self.synchronize_interventions()?;
            return Ok(WatchdogDisposition::WaitingForHuman);
        }
        if !all_done {
            let reopen_count = {
                let connection = self.store.connection.lock().unwrap();
                connection.query_row(
                    "SELECT COUNT(*) FROM ma_semantic_watchdogs
                     WHERE project_id = ?1 AND target_task_id = ?2 AND status = 'issues_found'",
                    params![project_id, target.id],
                    |row| row.get::<_, u32>(0),
                )?
            };
            if reopen_count > 0 {
                self.create_watchdog_attention(
                    project_id,
                    &target.id,
                    &fingerprint,
                    "语义复核已经自动重开过该任务，但任务树再次停止，需要人工决定下一步",
                )?;
                self.synchronize_interventions()?;
                return Ok(WatchdogDisposition::WaitingForHuman);
            }
        }

        self.schedule_watchdog(&snapshot, &watchdog_tasks, &target.id, &fingerprint)?;
        Ok(WatchdogDisposition::Scheduled)
    }

    pub fn is_watchdog_task(&self, task_id: &str) -> Result<bool> {
        let connection = self.store.connection.lock().unwrap();
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM ma_semantic_watchdogs WHERE review_task_id = ?1)",
            params![task_id],
            |row| row.get(0),
        )?)
    }

    fn respond_to_waiting_run(
        &self,
        intervention: &InterventionRecord,
        response: &str,
    ) -> Result<()> {
        let run_id = intervention
            .run_id
            .as_deref()
            .ok_or_else(|| anyhow!("该干预事项没有等待回复的 Run"))?;
        let (project_id, task_id, agent_id, status) = {
            let connection = self.store.connection.lock().unwrap();
            connection
                .query_row(
                    "SELECT project_id, task_id, agent_id, status FROM ma_runs WHERE id = ?1",
                    params![run_id],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                        ))
                    },
                )
                .optional()?
                .ok_or_else(|| anyhow!("等待回复的 Run 已不存在"))?
        };
        if status != "waiting_input" {
            return Err(anyhow!("Agent 当前已不再等待输入"));
        }
        self.store
            .send_collaboration_message(SendCollaborationMessageInput {
                project_id: project_id.clone(),
                task_id: Some(task_id.clone()),
                sender_type: "user".to_string(),
                sender_id: None,
                target_agent_id: Some(agent_id.clone()),
                body: response.to_string(),
            })?;
        let now = now_millis();
        let connection = self.store.connection.lock().unwrap();
        connection.execute(
            "UPDATE ma_runs SET status = 'running', last_heartbeat_at = ?2,
                    lease_expires_at = MAX(lease_expires_at, ?3)
             WHERE id = ?1 AND status = 'waiting_input'",
            params![run_id, now, now.saturating_add(120_000)],
        )?;
        connection.execute(
            "UPDATE ma_tasks SET status = 'running', updated_at = ?2 WHERE id = ?1",
            params![task_id, now],
        )?;
        connection.execute(
            "UPDATE ma_agent_sessions SET status = 'running', updated_at = ?2
             WHERE current_run_id = ?1",
            params![run_id, now],
        )?;
        append_event(
            &connection,
            &project_id,
            "run",
            run_id,
            "run.input_supplied",
            serde_json::json!({"taskId": task_id, "agentId": agent_id}),
            now,
        )?;
        Ok(())
    }

    fn synchronize_interventions(&self) -> Result<()> {
        let now = now_millis();
        let connection = self.store.connection.lock().unwrap();
        connection.execute(
            "UPDATE ma_interventions
             SET status = 'open', snoozed_until = NULL, updated_at = ?1
             WHERE status = 'snoozed' AND snoozed_until <= ?1",
            params![now],
        )?;
        let mut candidates = Vec::new();

        {
            let mut statement = connection.prepare(
                "SELECT a.id, a.project_id, a.task_id, a.run_id, a.summary,
                        COALESCE(t.title, '项目审批')
                 FROM ma_approvals a
                 JOIN ma_projects p ON p.id = a.project_id
                 LEFT JOIN ma_tasks t ON t.id = a.task_id
                 WHERE a.status = 'pending' AND p.status IN ('active','paused')",
            )?;
            candidates.extend(
                statement
                    .query_map([], |row| {
                        let approval_id: String = row.get(0)?;
                        Ok(Candidate {
                            project_id: row.get(1)?,
                            task_id: row.get(2)?,
                            run_id: row.get(3)?,
                            kind: InterventionKind::ApprovalRequired,
                            severity: InterventionSeverity::High,
                            title: format!("等待批准：{}", row.get::<_, String>(5)?),
                            summary: row.get(4)?,
                            action: InterventionRecommendedAction::Approve,
                            fingerprint: format!("approval:{approval_id}"),
                            metadata: serde_json::json!({"approvalId": approval_id}),
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }

        {
            let mut statement = connection.prepare(
                "SELECT r.id, r.project_id, r.task_id, t.title, r.agent_id,
                        COALESCE(r.output_summary, r.error, 'Agent 正在等待补充信息')
                 FROM ma_runs r
                 JOIN ma_tasks t ON t.id = r.task_id
                 JOIN ma_projects p ON p.id = r.project_id
                 WHERE r.status = 'waiting_input' AND p.status IN ('active','paused')",
            )?;
            candidates.extend(
                statement
                    .query_map([], |row| {
                        let run_id: String = row.get(0)?;
                        Ok(Candidate {
                            project_id: row.get(1)?,
                            task_id: Some(row.get(2)?),
                            run_id: Some(run_id.clone()),
                            kind: InterventionKind::AgentInputRequired,
                            severity: InterventionSeverity::High,
                            title: format!("Agent 等待输入：{}", row.get::<_, String>(3)?),
                            summary: row.get(5)?,
                            action: InterventionRecommendedAction::Respond,
                            fingerprint: format!("input:{run_id}"),
                            metadata: serde_json::json!({"agentId": row.get::<_, String>(4)?}),
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }

        {
            let mut statement = connection.prepare(
                "SELECT t.id, t.project_id, t.title, t.updated_at,
                        COALESCE((SELECT r.error FROM ma_runs r WHERE r.task_id = t.id
                                  ORDER BY r.attempt DESC LIMIT 1), '任务已耗尽自动重试次数'),
                        COALESCE((SELECT MAX(r.attempt) FROM ma_runs r WHERE r.task_id = t.id), 0),
                        t.max_attempts
                 FROM ma_tasks t JOIN ma_projects p ON p.id = t.project_id
                 WHERE t.status = 'failed' AND p.status IN ('active','paused')
                   AND NOT EXISTS(SELECT 1 FROM ma_semantic_watchdogs w WHERE w.review_task_id = t.id)
                   AND NOT EXISTS(SELECT 1 FROM ma_semantic_watchdogs w
                                  WHERE w.target_task_id = t.id AND w.status = 'scheduled')",
            )?;
            candidates.extend(
                statement
                    .query_map([], |row| {
                        let task_id: String = row.get(0)?;
                        let updated_at: i64 = row.get(3)?;
                        Ok(Candidate {
                            project_id: row.get(1)?,
                            task_id: Some(task_id.clone()),
                            run_id: None,
                            kind: InterventionKind::RetryExhausted,
                            severity: InterventionSeverity::Critical,
                            title: format!("重试耗尽：{}", row.get::<_, String>(2)?),
                            summary: row.get(4)?,
                            action: InterventionRecommendedAction::Retry,
                            fingerprint: format!("failed:{task_id}:{updated_at}"),
                            metadata: serde_json::json!({
                                "attempt": row.get::<_, u32>(5)?,
                                "maxAttempts": row.get::<_, u32>(6)?,
                            }),
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }

        {
            let mut statement = connection.prepare(
                "SELECT p.id, MIN(t.id), COUNT(*), MAX(t.updated_at)
                 FROM ma_projects p JOIN ma_tasks t ON t.project_id = p.id
                 WHERE p.status IN ('active','paused') AND t.status = 'blocked'
                   AND NOT EXISTS(SELECT 1 FROM ma_runs r WHERE r.project_id = p.id
                                  AND r.status IN ('queued','starting','running','waiting_input','waiting_approval'))
                   AND NOT EXISTS(SELECT 1 FROM ma_tasks ready WHERE ready.project_id = p.id
                                  AND ready.status IN ('ready','claimed','running','changes_requested','waiting_approval','in_review'))
                 GROUP BY p.id",
            )?;
            candidates.extend(
                statement
                    .query_map([], |row| {
                        let project_id: String = row.get(0)?;
                        let task_id: String = row.get(1)?;
                        let count: u32 = row.get(2)?;
                        let updated_at: i64 = row.get(3)?;
                        Ok(Candidate {
                            project_id: project_id.clone(),
                            task_id: Some(task_id),
                            run_id: None,
                            kind: InterventionKind::DependencyBlocked,
                            severity: InterventionSeverity::Medium,
                            title: "任务依赖链已停止".to_string(),
                            summary: format!("{count} 个任务等待无法完成的前置依赖"),
                            action: InterventionRecommendedAction::OpenProject,
                            fingerprint: format!("blocked:{project_id}:{updated_at}"),
                            metadata: serde_json::json!({"blockedTaskCount": count}),
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }

        {
            let mut statement = connection.prepare(
                "SELECT w.id, w.project_id, w.review_task_id, w.target_task_id,
                        w.status, w.summary, w.updated_at
                 FROM ma_semantic_watchdogs w JOIN ma_projects p ON p.id = w.project_id
                 WHERE w.status IN ('failed','needs_human') AND p.status IN ('active','paused')",
            )?;
            candidates.extend(
                statement
                    .query_map([], |row| {
                        let watchdog_id: String = row.get(0)?;
                        let status: String = row.get(4)?;
                        let updated_at: i64 = row.get(6)?;
                        Ok(Candidate {
                            project_id: row.get(1)?,
                            task_id: Some(row.get(3)?),
                            run_id: None,
                            kind: InterventionKind::SemanticReview,
                            severity: InterventionSeverity::Critical,
                            title: "语义复核需要人工处理".to_string(),
                            summary: row.get(5)?,
                            action: InterventionRecommendedAction::Review,
                            fingerprint: format!("watchdog:{watchdog_id}:{status}:{updated_at}"),
                            metadata: serde_json::json!({
                                "watchdogId": watchdog_id,
                                "reviewTaskId": row.get::<_, String>(2)?,
                            }),
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?,
            );
        }

        let fingerprints = candidates
            .iter()
            .map(|candidate| candidate.fingerprint.clone())
            .collect::<HashSet<_>>();
        for candidate in candidates {
            connection.execute(
                "INSERT INTO ma_interventions
                 (id, project_id, task_id, run_id, kind, severity, title, summary, status,
                  recommended_action, fingerprint, metadata_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'open', ?9, ?10, ?11, ?12, ?12)
                 ON CONFLICT(fingerprint) DO UPDATE SET
                   project_id = excluded.project_id,
                   task_id = excluded.task_id,
                   run_id = excluded.run_id,
                   severity = excluded.severity,
                   title = excluded.title,
                   summary = excluded.summary,
                   recommended_action = excluded.recommended_action,
                   metadata_json = excluded.metadata_json,
                   status = CASE
                     WHEN ma_interventions.status = 'snoozed'
                          AND ma_interventions.snoozed_until > excluded.updated_at THEN 'snoozed'
                     WHEN ma_interventions.status = 'resolved' THEN 'resolved'
                     ELSE 'open' END,
                   updated_at = excluded.updated_at",
                params![
                    uuid::Uuid::new_v4().to_string(),
                    candidate.project_id,
                    candidate.task_id,
                    candidate.run_id,
                    candidate.kind.as_str(),
                    candidate.severity.as_str(),
                    candidate.title,
                    candidate.summary,
                    candidate.action.as_str(),
                    candidate.fingerprint,
                    candidate.metadata.to_string(),
                    now,
                ],
            )?;
        }

        let mut statement = connection.prepare(
            "SELECT id, fingerprint FROM ma_interventions WHERE status IN ('open','snoozed')",
        )?;
        let existing = statement
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        for (id, fingerprint) in existing {
            if !fingerprints.contains(&fingerprint) {
                connection.execute(
                    "UPDATE ma_interventions SET status = 'resolved', resolved_at = ?2,
                            snoozed_until = NULL, updated_at = ?2 WHERE id = ?1",
                    params![id, now],
                )?;
            }
        }
        Ok(())
    }

    fn schedule_watchdog(
        &self,
        snapshot: &MultiAgentSnapshot,
        watchdog_tasks: &HashSet<String>,
        target_task_id: &str,
        fingerprint: &str,
    ) -> Result<()> {
        let context = semantic_context(snapshot, watchdog_tasks, target_task_id);
        let mut connection = self.store.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let reviewer_id = ensure_reviewer(&tx, &snapshot.project.id)?;
        let review_task_id = uuid::Uuid::new_v4().to_string();
        let watchdog_id = uuid::Uuid::new_v4().to_string();
        let now = now_millis();
        let stage: u32 = tx.query_row(
            "SELECT COALESCE(MAX(stage), 0) + 1 FROM ma_tasks WHERE project_id = ?1",
            params![snapshot.project.id],
            |row| row.get(0),
        )?;
        tx.execute(
            "INSERT INTO ma_tasks
             (id, project_id, parent_id, stage, title, description, acceptance_criteria,
              verification_command, status, assignee_agent_id, reviewer_agent_id,
              workspace_mode, priority, required_capabilities_json, max_attempts,
              retry_backoff_seconds, next_retry_at, created_at, updated_at)
             VALUES (?1, ?2, NULL, ?3, '项目语义复核', ?4, ?5, NULL, 'ready', ?6, NULL,
                     'read_only', 1000, '[]', 2, 5, NULL, ?7, ?7)",
            params![
                review_task_id,
                snapshot.project.id,
                stage,
                context,
                format!(
                    "对照项目目标复核全部任务与交付证据，并通过 cc_review_submit 提交 target_task_id={} 的结构化结论",
                    target_task_id
                ),
                reviewer_id,
                now,
            ],
        )?;
        tx.execute(
            "INSERT INTO ma_semantic_watchdogs
             (id, project_id, review_task_id, target_task_id, reviewer_agent_id,
              fingerprint, status, summary, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'scheduled', '', ?7, ?7)",
            params![
                watchdog_id,
                snapshot.project.id,
                review_task_id,
                target_task_id,
                reviewer_id,
                fingerprint,
                now,
            ],
        )?;
        append_event(
            &tx,
            &snapshot.project.id,
            "watchdog",
            &watchdog_id,
            "watchdog.scheduled",
            serde_json::json!({
                "reviewTaskId": review_task_id,
                "targetTaskId": target_task_id,
                "fingerprint": fingerprint,
            }),
            now,
        )?;
        tx.commit()?;
        Ok(())
    }

    fn mark_watchdog_needs_human(
        &self,
        watchdog_id: &str,
        summary: &str,
        status: &str,
    ) -> Result<()> {
        let connection = self.store.connection.lock().unwrap();
        connection.execute(
            "UPDATE ma_semantic_watchdogs
             SET status = ?2, summary = ?3, updated_at = ?4 WHERE id = ?1",
            params![watchdog_id, status, summary, now_millis()],
        )?;
        Ok(())
    }

    fn create_watchdog_attention(
        &self,
        project_id: &str,
        target_task_id: &str,
        fingerprint: &str,
        summary: &str,
    ) -> Result<()> {
        let connection = self.store.connection.lock().unwrap();
        let existing: Option<(String, String)> = connection
            .query_row(
                "SELECT id, review_task_id FROM ma_semantic_watchdogs
                 WHERE project_id = ?1 AND target_task_id = ?2
                 ORDER BY created_at DESC LIMIT 1",
                params![project_id, target_task_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((watchdog_id, _)) = existing else {
            return Err(anyhow!("无法建立语义复核人工干预记录"));
        };
        connection.execute(
            "UPDATE ma_semantic_watchdogs
             SET fingerprint = ?2, status = 'needs_human', summary = ?3, updated_at = ?4
             WHERE id = ?1",
            params![watchdog_id, fingerprint, summary, now_millis()],
        )?;
        Ok(())
    }
}

pub(super) fn watchdog_target(
    connection: &Connection,
    review_task_id: &str,
) -> Result<Option<String>> {
    Ok(connection
        .query_row(
            "SELECT target_task_id FROM ma_semantic_watchdogs WHERE review_task_id = ?1",
            params![review_task_id],
            |row| row.get(0),
        )
        .optional()?)
}

pub(super) fn apply_watchdog_decision(
    connection: &Connection,
    decision: &super::ReviewDecision,
    target: &super::MultiAgentTask,
    now: i64,
) -> Result<bool> {
    let watchdog_id: Option<String> = connection
        .query_row(
            "SELECT id FROM ma_semantic_watchdogs WHERE review_task_id = ?1",
            params![decision.review_task_id],
            |row| row.get(0),
        )
        .optional()?;
    let Some(watchdog_id) = watchdog_id else {
        return Ok(false);
    };
    match decision.verdict {
        super::ReviewVerdict::Approved => {
            if target.status != TaskStatus::Done {
                connection.execute(
                    "UPDATE ma_tasks SET status = 'done', next_retry_at = NULL, updated_at = ?2
                     WHERE id = ?1",
                    params![target.id, now],
                )?;
                unblock_ready_tasks(connection, &target.project_id, now)?;
            }
            connection.execute(
                "UPDATE ma_semantic_watchdogs
                 SET status = 'passed', summary = ?2, reviewed_at = ?3, updated_at = ?3
                 WHERE id = ?1",
                params![watchdog_id, decision.summary, now],
            )?;
            append_event(
                connection,
                &decision.project_id,
                "watchdog",
                &watchdog_id,
                "watchdog.passed",
                serde_json::json!({"targetTaskId": target.id, "summary": decision.summary}),
                now,
            )?;
        }
        super::ReviewVerdict::ChangesRequested => {
            let latest_attempt: u32 = connection.query_row(
                "SELECT COALESCE(MAX(attempt), 0) FROM ma_runs WHERE task_id = ?1",
                params![target.id],
                |row| row.get(0),
            )?;
            connection.execute(
                "UPDATE ma_tasks
                 SET status = 'changes_requested', next_retry_at = NULL,
                     max_attempts = MAX(max_attempts, ?2), updated_at = ?3
                 WHERE id = ?1",
                params![target.id, latest_attempt.saturating_add(1), now],
            )?;
            connection.execute(
                "UPDATE ma_semantic_watchdogs
                 SET status = 'issues_found', summary = ?2, reviewed_at = ?3, updated_at = ?3
                 WHERE id = ?1",
                params![watchdog_id, decision.summary, now],
            )?;
            append_event(
                connection,
                &decision.project_id,
                "watchdog",
                &watchdog_id,
                "watchdog.reopened_task",
                serde_json::json!({"targetTaskId": target.id, "summary": decision.summary}),
                now,
            )?;
        }
    }
    Ok(true)
}

fn choose_watchdog_target<'a>(
    snapshot: &'a MultiAgentSnapshot,
    watchdog_tasks: &HashSet<String>,
) -> Option<&'a super::MultiAgentTask> {
    let rank = |status: TaskStatus| match status {
        TaskStatus::Failed => 0,
        TaskStatus::ChangesRequested => 1,
        TaskStatus::Cancelled => 2,
        TaskStatus::Blocked => 3,
        TaskStatus::InReview | TaskStatus::WaitingApproval => 4,
        TaskStatus::Ready | TaskStatus::Claimed | TaskStatus::Running => 5,
        TaskStatus::Done => 6,
        TaskStatus::Draft => 7,
    };
    let integrators = snapshot
        .agents
        .iter()
        .filter(|agent| agent.role == super::AgentRole::Integrator)
        .map(|agent| agent.id.as_str())
        .collect::<HashSet<_>>();
    let mut tasks = snapshot
        .tasks
        .iter()
        .filter(|task| !watchdog_tasks.contains(&task.id) && !is_communication_task(task))
        .collect::<Vec<_>>();
    tasks.sort_by_key(|task| {
        (
            rank(task.status),
            if task
                .assignee_agent_id
                .as_deref()
                .is_some_and(|id| integrators.contains(id))
            {
                0
            } else {
                1
            },
            std::cmp::Reverse(task.stage),
            std::cmp::Reverse(task.updated_at),
        )
    });
    tasks.into_iter().next()
}

fn is_communication_task(task: &super::MultiAgentTask) -> bool {
    // Only the dedicated delivery queue marks a task as pure communication.
    // Role, workspace mode and a CLI contract are also valid for business tasks.
    task.id.starts_with("cli-message-")
}

fn semantic_fingerprint(snapshot: &MultiAgentSnapshot, watchdog_tasks: &HashSet<String>) -> String {
    let mut state = std::collections::hash_map::DefaultHasher::new();
    snapshot.project.id.hash(&mut state);
    snapshot.project.goal.hash(&mut state);
    let mut tasks = snapshot
        .tasks
        .iter()
        .filter(|task| !watchdog_tasks.contains(&task.id) && !is_communication_task(task))
        .collect::<Vec<_>>();
    tasks.sort_by(|left, right| left.id.cmp(&right.id));
    for task in tasks {
        task.id.hash(&mut state);
        task.status.as_str().hash(&mut state);
        task.updated_at.hash(&mut state);
    }
    let mut artifacts = snapshot
        .artifacts
        .iter()
        .filter(|artifact| {
            !watchdog_tasks.contains(&artifact.task_id)
                && !snapshot
                    .tasks
                    .iter()
                    .any(|task| task.id == artifact.task_id && is_communication_task(task))
        })
        .collect::<Vec<_>>();
    artifacts.sort_by(|left, right| left.id.cmp(&right.id));
    for artifact in artifacts {
        artifact.task_id.hash(&mut state);
        artifact.content_hash.hash(&mut state);
        artifact.summary.hash(&mut state);
    }
    format!("semantic:{:016x}", state.finish())
}

fn semantic_context(
    snapshot: &MultiAgentSnapshot,
    watchdog_tasks: &HashSet<String>,
    target_task_id: &str,
) -> String {
    let agents = snapshot
        .agents
        .iter()
        .map(|agent| (agent.id.as_str(), agent.name.as_str()))
        .collect::<HashMap<_, _>>();
    let mut lines = vec![
        "这是调度器在整棵任务树停止后启动的独立语义复核。不要相信“完成”标签本身，必须检查项目目标、验收标准、代码与交付证据是否一致。".to_string(),
        format!("项目目标：{}", snapshot.project.goal),
        format!("结构化结论必须使用 target_task_id={target_task_id}。"),
        "若整体目标已有充分证据达成，提交 approved；若存在遗漏、假完成、失败或阻塞，提交 changes_requested，并给出可执行修复说明。".to_string(),
        "任务与证据：".to_string(),
    ];
    for task in snapshot
        .tasks
        .iter()
        .filter(|task| !watchdog_tasks.contains(&task.id) && !is_communication_task(task))
    {
        let latest_run = snapshot
            .runs
            .iter()
            .filter(|run| run.task_id == task.id)
            .max_by_key(|run| run.attempt);
        let artifacts = snapshot
            .artifacts
            .iter()
            .filter(|artifact| artifact.task_id == task.id)
            .map(|artifact| artifact.summary.as_str())
            .collect::<Vec<_>>()
            .join("；");
        lines.push(format!(
            "- [{}] {}｜Agent={}｜验收={}｜结果={}｜错误={}｜交付={}",
            task.status.as_str(),
            task.title,
            task.assignee_agent_id
                .as_deref()
                .and_then(|id| agents.get(id).copied())
                .unwrap_or("未指派"),
            task.acceptance_criteria,
            latest_run
                .and_then(|run| run.output_summary.as_deref())
                .unwrap_or("无"),
            latest_run
                .and_then(|run| run.error.as_deref())
                .unwrap_or("无"),
            if artifacts.is_empty() {
                "无"
            } else {
                &artifacts
            },
        ));
    }
    let mut value = lines.join("\n");
    if value.len() > 16_000 {
        let mut end = 16_000;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
    }
    value
}

fn ensure_reviewer(connection: &Connection, project_id: &str) -> Result<String> {
    if let Some(id) = connection
        .query_row(
            "SELECT id FROM ma_agents
             WHERE project_id = ?1 AND role = 'reviewer' AND enabled = 1
             ORDER BY updated_at DESC LIMIT 1",
            params![project_id],
            |row| row.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let source = connection
        .query_row(
            "SELECT runtime_kind, model, permission_profile, capabilities_json
             FROM ma_agents WHERE project_id = ?1 AND enabled = 1
             ORDER BY CASE role WHEN 'orchestrator' THEN 1 ELSE 0 END, updated_at DESC LIMIT 1",
            params![project_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| anyhow!("项目没有可用于语义复核的 Agent Runtime"))?;
    let id = uuid::Uuid::new_v4().to_string();
    let now = now_millis();
    connection.execute(
        "INSERT INTO ma_agents
         (id, project_id, name, runtime_kind, role, description, model,
          permission_profile, max_concurrency, capabilities_json, enabled, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'reviewer', ?5, ?6, ?7, 1, ?8, 1, ?9, ?9)",
        params![
            id,
            project_id,
            format!("语义复核员-{}", &id[..6]),
            source.0,
            "独立检查项目目标、任务状态与交付证据",
            source.1,
            source.2,
            source.3,
            now,
        ],
    )?;
    Ok(id)
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
    payload: serde_json::Value,
    created_at: i64,
) -> Result<()> {
    connection.execute(
        "INSERT INTO ma_events
         (project_id, entity_type, entity_id, event_type, actor_type, actor_id,
          payload_json, created_at)
         VALUES (?1, ?2, ?3, ?4, 'system', NULL, ?5, ?6)",
        params![
            project_id,
            entity_type,
            entity_id,
            event_type,
            payload.to_string(),
            created_at
        ],
    )?;
    Ok(())
}

fn get_intervention(connection: &Connection, id: &str) -> Result<InterventionRecord> {
    connection
        .query_row(
            "SELECT i.id, i.project_id, p.name, i.task_id, i.run_id, i.kind, i.severity,
                    i.title, i.summary, i.status, i.recommended_action, i.fingerprint,
                    i.snoozed_until, i.metadata_json, i.created_at, i.updated_at, i.resolved_at
             FROM ma_interventions i JOIN ma_projects p ON p.id = i.project_id
             WHERE i.id = ?1",
            params![id],
            intervention_from_row,
        )
        .optional()?
        .ok_or_else(|| anyhow!("干预事项不存在或已被清理"))
}

fn intervention_from_row(row: &Row<'_>) -> rusqlite::Result<InterventionRecord> {
    let metadata_json: String = row.get(13)?;
    Ok(InterventionRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        project_name: row.get(2)?,
        task_id: row.get(3)?,
        run_id: row.get(4)?,
        kind: parse_kind(row.get(5)?)?,
        severity: parse_severity(row.get(6)?)?,
        title: row.get(7)?,
        summary: row.get(8)?,
        status: parse_status(row.get(9)?)?,
        recommended_action: parse_action(row.get(10)?)?,
        fingerprint: row.get(11)?,
        snoozed_until: row.get(12)?,
        metadata: serde_json::from_str(&metadata_json).unwrap_or_else(|_| serde_json::json!({})),
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
        resolved_at: row.get(16)?,
    })
}

fn parse_kind(value: String) -> rusqlite::Result<InterventionKind> {
    match value.as_str() {
        "approval_required" => Ok(InterventionKind::ApprovalRequired),
        "agent_input_required" => Ok(InterventionKind::AgentInputRequired),
        "retry_exhausted" => Ok(InterventionKind::RetryExhausted),
        "dependency_blocked" => Ok(InterventionKind::DependencyBlocked),
        "semantic_review" => Ok(InterventionKind::SemanticReview),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_severity(value: String) -> rusqlite::Result<InterventionSeverity> {
    match value.as_str() {
        "critical" => Ok(InterventionSeverity::Critical),
        "high" => Ok(InterventionSeverity::High),
        "medium" => Ok(InterventionSeverity::Medium),
        "low" => Ok(InterventionSeverity::Low),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_status(value: String) -> rusqlite::Result<InterventionStatus> {
    match value.as_str() {
        "open" => Ok(InterventionStatus::Open),
        "snoozed" => Ok(InterventionStatus::Snoozed),
        "resolved" => Ok(InterventionStatus::Resolved),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn parse_action(value: String) -> rusqlite::Result<InterventionRecommendedAction> {
    match value.as_str() {
        "approve" => Ok(InterventionRecommendedAction::Approve),
        "respond" => Ok(InterventionRecommendedAction::Respond),
        "retry" => Ok(InterventionRecommendedAction::Retry),
        "open_project" => Ok(InterventionRecommendedAction::OpenProject),
        "review" => Ok(InterventionRecommendedAction::Review),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multi_agent::{
        AgentRole, AgentRuntimeKind, ClaimTaskInput, CreateAgentInput, CreateProjectInput,
        CreateTaskInput, InterventionAction, PermissionProfile, ResolveInterventionInput,
        ReviewVerdict, RunStatus, SubmitReviewDecisionInput, TaskStatus, UpdateRunInput,
        WorkspaceMode,
    };

    struct Fixture {
        store: MultiAgentStore,
        root: std::path::PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("crewvela-governance-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let store = MultiAgentStore::open_at(root.join("multi-agent.db")).unwrap();
            Self { store, root }
        }

        fn project_and_agent(
            &self,
            role: AgentRole,
        ) -> (super::super::MultiAgentProject, super::super::AgentProfile) {
            let project = self
                .store
                .create_project(CreateProjectInput {
                    name: "治理测试".to_string(),
                    goal: "交付可验证结果".to_string(),
                    root_path: self.root.join("project").display().to_string(),
                    default_branch: "main".to_string(),
                })
                .unwrap();
            let agent = self
                .store
                .create_agent(CreateAgentInput {
                    avatar_id: None,
                    project_id: project.id.clone(),
                    name: "Agent".to_string(),
                    runtime_kind: AgentRuntimeKind::ClaudeCode,
                    role,
                    description: "测试".to_string(),
                    model: None,
                    permission_profile: PermissionProfile::Default,
                    max_concurrency: 1,
                
                persona: Default::default(),
                persona_custom: String::new(),})
                .unwrap();
            (project, agent)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn exhausted_failure_is_deduplicated_in_global_inbox() {
        let fixture = Fixture::new();
        let (project, agent) = fixture.project_and_agent(AgentRole::Worker);
        let task = fixture
            .store
            .create_task(CreateTaskInput {
                project_id: project.id,
                parent_id: None,
                stage: 0,
                title: "实现".to_string(),
                description: String::new(),
                acceptance_criteria: "通过".to_string(),
                verification_command: None,
                assignee_agent_id: Some(agent.id),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::ReadOnly,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 1,
                retry_backoff_seconds: 1,
            })
            .unwrap();
        let connection = fixture.store.connection.lock().unwrap();
        connection
            .execute(
                "UPDATE ma_tasks SET status = 'failed', updated_at = 42 WHERE id = ?1",
                params![task.id],
            )
            .unwrap();
        drop(connection);
        let governance = CollaborationGovernance::new(fixture.store.clone());
        let first = governance.list_interventions().unwrap();
        let second = governance.list_interventions().unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].id, second[0].id);
        assert_eq!(first[0].kind, InterventionKind::RetryExhausted);

        let receipt = governance
            .resolve(ResolveInterventionInput {
                intervention_id: first[0].id.clone(),
                action: InterventionAction::Retry,
                response: None,
                agent_id: None,
                snooze_minutes: None,
            })
            .unwrap();
        assert!(receipt.resume_project);
        assert_eq!(
            fixture
                .store
                .snapshot(&receipt.project_id)
                .unwrap()
                .tasks
                .into_iter()
                .find(|candidate| candidate.id == task.id)
                .unwrap()
                .status,
            TaskStatus::Ready
        );
        assert!(governance.list_interventions().unwrap().is_empty());
    }

    #[test]
    fn replying_to_waiting_agent_resumes_run_and_clears_intervention() {
        let fixture = Fixture::new();
        let (project, agent) = fixture.project_and_agent(AgentRole::Worker);
        let task = fixture
            .store
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "等待用户选择".to_string(),
                description: String::new(),
                acceptance_criteria: "收到明确选择".to_string(),
                verification_command: None,
                assignee_agent_id: Some(agent.id.clone()),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::ReadOnly,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 1,
                retry_backoff_seconds: 1,
            })
            .unwrap();
        let run = fixture
            .store
            .claim_task(ClaimTaskInput {
                task_id: task.id,
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store
            .update_run(UpdateRunInput {
                run_id: run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store
            .update_run(UpdateRunInput {
                run_id: run.id.clone(),
                status: RunStatus::WaitingInput,
                output_summary: Some("请选择发布环境".to_string()),
                error: None,
                session_locator: None,
            })
            .unwrap();

        let governance = CollaborationGovernance::new(fixture.store.clone());
        let intervention = governance.list_interventions().unwrap().remove(0);
        assert_eq!(intervention.kind, InterventionKind::AgentInputRequired);
        governance
            .resolve(ResolveInterventionInput {
                intervention_id: intervention.id,
                action: InterventionAction::Respond,
                response: Some("发布到预发布环境".to_string()),
                agent_id: None,
                snooze_minutes: None,
            })
            .unwrap();

        let snapshot = fixture.store.snapshot(&project.id).unwrap();
        assert_eq!(snapshot.runs[0].status, RunStatus::Running);
        assert!(snapshot.messages.iter().any(|message| {
            message.sender_type == "user" && message.body == "发布到预发布环境"
        }));
        assert!(governance.list_interventions().unwrap().is_empty());
    }

    #[test]
    fn quiescent_completed_tree_schedules_exactly_one_semantic_review() {
        let fixture = Fixture::new();
        let (project, agent) = fixture.project_and_agent(AgentRole::Worker);
        let task = fixture
            .store
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "实现".to_string(),
                description: String::new(),
                acceptance_criteria: "通过".to_string(),
                verification_command: None,
                assignee_agent_id: Some(agent.id),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::ReadOnly,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 1,
                retry_backoff_seconds: 1,
            })
            .unwrap();
        let connection = fixture.store.connection.lock().unwrap();
        connection
            .execute(
                "UPDATE ma_tasks SET status = 'done' WHERE id = ?1",
                params![task.id],
            )
            .unwrap();
        drop(connection);
        let governance = CollaborationGovernance::new(fixture.store.clone());
        assert_eq!(
            governance.evaluate_quiescence(&project.id).unwrap(),
            WatchdogDisposition::Scheduled
        );
        let snapshot = fixture.store.snapshot(&project.id).unwrap();
        assert_eq!(snapshot.tasks.len(), 2);
        assert_eq!(
            snapshot
                .agents
                .iter()
                .filter(|item| item.role == AgentRole::Reviewer)
                .count(),
            1
        );
        assert_eq!(
            governance.evaluate_quiescence(&project.id).unwrap(),
            WatchdogDisposition::Scheduled
        );
        let snapshot = fixture.store.snapshot(&project.id).unwrap();
        assert_eq!(snapshot.tasks.len(), 2);
        let review_task = snapshot
            .tasks
            .iter()
            .find(|candidate| governance.is_watchdog_task(&candidate.id).unwrap())
            .unwrap();
        let review_run = fixture
            .store
            .claim_task(ClaimTaskInput {
                task_id: review_task.id.clone(),
                agent_id: review_task.assignee_agent_id.clone().unwrap(),
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store
            .update_run(UpdateRunInput {
                run_id: review_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store
            .submit_review_decision(SubmitReviewDecisionInput {
                review_run_id: review_run.id.clone(),
                target_task_id: task.id,
                verdict: ReviewVerdict::Approved,
                summary: "证据充分".to_string(),
            })
            .unwrap();
        fixture
            .store
            .finalize_successful_run(
                &review_run.id,
                &project.root_path,
                "语义复核通过",
                true,
                None,
                None,
            )
            .unwrap();
        assert_eq!(
            governance.evaluate_quiescence(&project.id).unwrap(),
            WatchdogDisposition::ClearToComplete
        );
        assert_eq!(fixture.store.snapshot(&project.id).unwrap().tasks.len(), 2);
    }

    #[test]
    fn quiescent_fully_failed_tree_asks_for_human_instead_of_semantic_review() {
        let fixture = Fixture::new();
        let (project, agent) = fixture.project_and_agent(AgentRole::Worker);
        let task = fixture
            .store
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "打招呼".to_string(),
                description: String::new(),
                acceptance_criteria: "送达".to_string(),
                verification_command: None,
                assignee_agent_id: Some(agent.id),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::ReadOnly,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 1,
                retry_backoff_seconds: 1,
            })
            .unwrap();
        {
            let connection = fixture.store.connection.lock().unwrap();
            connection
                .execute(
                    "UPDATE ma_tasks SET status = 'failed' WHERE id = ?1",
                    params![task.id],
                )
                .unwrap();
        }
        let governance = CollaborationGovernance::new(fixture.store.clone());
        assert_eq!(
            governance.evaluate_quiescence(&project.id).unwrap(),
            WatchdogDisposition::WaitingForHuman
        );
        let snapshot = fixture.store.snapshot(&project.id).unwrap();
        assert!(
            !snapshot
                .tasks
                .iter()
                .any(|candidate| candidate.title == "项目语义复核"),
            "全失败任务树不应自动创建语义复核任务"
        );
    }

    #[test]
    fn integrator_runtime_approval_never_attempts_delivery_merge() {
        let fixture = Fixture::new();
        let (project, agent) = fixture.project_and_agent(AgentRole::Integrator);
        let task = fixture.store.create_task(CreateTaskInput {
            project_id: project.id.clone(), parent_id: None, stage: 1, title: "交付集成".into(), description: String::new(), acceptance_criteria: "可验证交付".into(),
            verification_command: None, assignee_agent_id: Some(agent.id.clone()), reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::Worktree, priority: 0, required_capabilities: Vec::new(), max_attempts: 1, retry_backoff_seconds: 1,
        }).unwrap();
        let run = fixture.store.claim_task(ClaimTaskInput { task_id: task.id.clone(), agent_id: agent.id, lease_seconds: 60 }).unwrap();
        let first = fixture.store.request_approval(super::super::RequestApprovalInput { project_id: project.id.clone(), task_id: Some(task.id.clone()), run_id: Some(run.id.clone()), approval_type: "runtime_permission".into(), summary: "允许测试命令".into() }).unwrap();
        let second = fixture.store.request_approval(super::super::RequestApprovalInput { project_id: project.id.clone(), task_id: Some(task.id.clone()), run_id: Some(run.id.clone()), approval_type: "user_confirmation".into(), summary: "另一个请求".into() }).unwrap();
        assert!(fixture.store.integrator_delivery(&task.id).unwrap().is_none(), "运行中请求不能触发交付合并");
        assert!(fixture.store.approve_task(&task.id).is_err(), "任务级批准不能批量放行不同审批");
        let governance = CollaborationGovernance::new(fixture.store.clone());
        let intervention = governance.list_interventions().unwrap().into_iter().find(|item| item.metadata["approvalId"] == first.id).unwrap();
        governance.resolve(ResolveInterventionInput { intervention_id: intervention.id, action: InterventionAction::Approve, response: None, agent_id: None, snooze_minutes: None }).unwrap();
        let snapshot = fixture.store.snapshot(&project.id).unwrap();
        assert_eq!(snapshot.tasks.iter().find(|item| item.id == task.id).unwrap().status, TaskStatus::WaitingApproval);
        assert_eq!(snapshot.approvals.iter().find(|item| item.id == first.id).unwrap().status, "approved");
        assert_eq!(snapshot.approvals.iter().find(|item| item.id == second.id).unwrap().status, "pending");
        let intervention = governance.list_interventions().unwrap().into_iter().find(|item| item.metadata["approvalId"] == second.id).unwrap();
        governance.resolve(ResolveInterventionInput { intervention_id: intervention.id, action: InterventionAction::Approve, response: None, agent_id: None, snooze_minutes: None }).unwrap();
        assert_eq!(fixture.store.snapshot(&project.id).unwrap().tasks.iter().find(|item| item.id == task.id).unwrap().status, TaskStatus::Running);
        // A delivery approval still validates the required artifact before completing.
        fixture.store.connection.lock().unwrap().execute("UPDATE ma_tasks SET status='in_review' WHERE id=?1", params![task.id]).unwrap();
        let delivery = fixture.store.request_approval(super::super::RequestApprovalInput { project_id: project.id.clone(), task_id: Some(task.id.clone()), run_id: None, approval_type: "task_review".into(), summary: "审核交付".into() }).unwrap();
        let intervention = governance.list_interventions().unwrap().into_iter().find(|item| item.metadata["approvalId"] == delivery.id).unwrap();
        assert!(governance.resolve(ResolveInterventionInput { intervention_id: intervention.id, action: InterventionAction::Approve, response: None, agent_id: None, snooze_minutes: None }).is_err());
    }

    #[test]
    fn business_tasks_with_cli_contract_still_require_semantic_review() {
        let fixture = Fixture::new();
        let (project, agent) = fixture.project_and_agent(AgentRole::Worker);
        let development = fixture.store.create_task(CreateTaskInput {
            project_id: project.id.clone(), parent_id: None, stage: 1,
            title: "开发登录功能".into(),
            description: "开发登录；通过 CREWVELA_CLI sessions 发现同伴并 send --to 协作".into(),
            acceptance_criteria: "功能通过；使用 CREWVELA_CLI send --to 同步结果".into(),
            verification_command: None, assignee_agent_id: Some(agent.id.clone()), reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::Worktree, priority: 0, required_capabilities: Vec::new(), max_attempts: 1, retry_backoff_seconds: 1,
        }).unwrap();
        fixture.store.connection.lock().unwrap().execute("UPDATE ma_tasks SET status='done' WHERE id=?1", params![development.id]).unwrap();
        let governance = CollaborationGovernance::new(fixture.store.clone());
        assert_eq!(governance.evaluate_quiescence(&project.id).unwrap(), WatchdogDisposition::Scheduled);
        let mut snapshot = fixture.store.snapshot(&project.id).unwrap();
        let mut review = development.clone();
        review.id = "business-review".into(); review.title = "登录安全审查".into(); review.workspace_mode = WorkspaceMode::ReadOnly;
        snapshot.tasks.push(review.clone());
        assert!(!is_communication_task(&review));
        snapshot.agents.iter_mut().find(|candidate| candidate.id == agent.id).unwrap().role = AgentRole::Orchestrator;
        assert!(!is_communication_task(&review), "architect business review must not be classified by role or CLI text");
        let context = semantic_context(&snapshot, &HashSet::new(), &development.id);
        assert!(context.contains(&development.title));
        assert!(context.contains(&review.title));
    }

    #[test]
    fn communication_only_tree_never_schedules_semantic_review() {
        let fixture = Fixture::new();
        let (project, agent) = fixture.project_and_agent(AgentRole::Orchestrator);
        let make_task = |title: &str| {
            let mut task = fixture
                .store
                .create_task(CreateTaskInput {
                    project_id: project.id.clone(),
                    parent_id: None,
                    stage: 0,
                    title: title.to_string(),
                    description: "运行 CREWVELA_CLI sessions 发现目标，再 send --to <目标> 发送问候"
                        .to_string(),
                    acceptance_criteria: "通过 CREWVELA_CLI send --to 送达".to_string(),
                    verification_command: None,
                    assignee_agent_id: Some(agent.id.clone()),
                    reviewer_agent_id: None,
                    workspace_mode: WorkspaceMode::ReadOnly,
                    priority: 0,
                    required_capabilities: Vec::new(),
                    max_attempts: 3,
                    retry_backoff_seconds: 5,
                })
                .unwrap();
            let id = format!("cli-message-{}", task.id);
            fixture.store.connection.lock().unwrap().execute("UPDATE ma_tasks SET id=?2 WHERE id=?1", params![task.id, id]).unwrap();
            task.id = id;
            task
        };
        let done_task = make_task("OpenCode 向 Claude 打招呼");
        let failed_task = make_task("Claude 向 OpenCode 打招呼");
        {
            let connection = fixture.store.connection.lock().unwrap();
            connection
                .execute(
                    "UPDATE ma_tasks SET status = 'done' WHERE id = ?1",
                    params![done_task.id],
                )
                .unwrap();
            connection
                .execute(
                    "UPDATE ma_tasks SET status = 'failed' WHERE id = ?1",
                    params![failed_task.id],
                )
                .unwrap();
        }
        let governance = CollaborationGovernance::new(fixture.store.clone());
        assert_eq!(
            governance.evaluate_quiescence(&project.id).unwrap(),
            WatchdogDisposition::WaitingForHuman
        );
        let snapshot = fixture.store.snapshot(&project.id).unwrap();
        assert!(
            !snapshot
                .tasks
                .iter()
                .any(|candidate| candidate.title == "项目语义复核"),
            "纯通信任务树不应调度语义复核"
        );
        // Legacy coordinator communication tasks can still be awaiting manual delivery review.
        fixture
            .store
            .connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE ma_tasks SET status = CASE WHEN id = ?1 THEN 'in_review' ELSE 'done' END",
                params![done_task.id],
            )
            .unwrap();
        fixture
            .store
            .request_approval(super::super::RequestApprovalInput {
                project_id: project.id.clone(),
                task_id: Some(done_task.id),
                run_id: None,
                approval_type: "task_review".into(),
                summary: "批准通信交付继续".into(),
            })
            .unwrap();
        assert_eq!(
            governance.evaluate_quiescence(&project.id).unwrap(),
            WatchdogDisposition::WaitingForHuman
        );
        assert!(!fixture
            .store
            .snapshot(&project.id)
            .unwrap()
            .tasks
            .iter()
            .any(|candidate| candidate.title == "项目语义复核"));
    }

    #[test]
    fn communication_in_mixed_project_does_not_change_review_target_or_fingerprint() {
        let fixture = Fixture::new();
        let (project, agent) = fixture.project_and_agent(AgentRole::Worker);
        let normal = fixture
            .store
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "实现首页".into(),
                description: "实现页面".into(),
                acceptance_criteria: "构建通过".into(),
                verification_command: None,
                assignee_agent_id: Some(agent.id),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::Worktree,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 1,
                retry_backoff_seconds: 1,
            })
            .unwrap();
        let mut snapshot = fixture.store.snapshot(&project.id).unwrap();
        let watchdogs = HashSet::new();
        let before = semantic_fingerprint(&snapshot, &watchdogs);
        let mut greeting = normal.clone();
        let mut messenger = snapshot.agents[0].clone();
        messenger.id = "legacy-messenger".into();
        messenger.role = AgentRole::Orchestrator;
        greeting.assignee_agent_id = Some(messenger.id.clone());
        snapshot.agents.push(messenger);
        greeting.id = "cli-message-greeting".into();
        greeting.title = "Pi 向 OpenCode 打招呼".into();
        greeting.description = "运行 CREWVELA_CLI sessions 后 send --to 发消息".into();
        greeting.workspace_mode = WorkspaceMode::ReadOnly;
        greeting.status = TaskStatus::Done;
        greeting.updated_at += 1000;
        snapshot.tasks.push(greeting.clone());
        snapshot.artifacts.push(super::super::ArtifactRecord {
            id: "greeting-artifact".into(),
            project_id: project.id,
            task_id: greeting.id.clone(),
            run_id: "greeting-run".into(),
            artifact_type: "review".into(),
            uri: "workspace".into(),
            summary: "问候已发送".into(),
            content_hash: None,
            created_at: 123,
        });
        assert_eq!(before, semantic_fingerprint(&snapshot, &watchdogs));
        assert_eq!(
            choose_watchdog_target(&snapshot, &watchdogs).unwrap().id,
            normal.id
        );
        assert!(!semantic_context(&snapshot, &watchdogs, &normal.id).contains(&greeting.title));
        snapshot.tasks.last_mut().unwrap().status = TaskStatus::Failed;
        assert_eq!(before, semantic_fingerprint(&snapshot, &watchdogs));
        snapshot.tasks[0].updated_at += 1;
        assert_ne!(
            before,
            semantic_fingerprint(&snapshot, &watchdogs),
            "真实交付变化必须重新复核"
        );
    }

    #[test]
    fn semantic_review_reopens_the_target_with_one_extra_attempt() {
        let fixture = Fixture::new();
        let (project, agent) = fixture.project_and_agent(AgentRole::Worker);
        let task = fixture
            .store
            .create_task(CreateTaskInput {
                project_id: project.id.clone(),
                parent_id: None,
                stage: 0,
                title: "实现".to_string(),
                description: String::new(),
                acceptance_criteria: "通过".to_string(),
                verification_command: None,
                assignee_agent_id: Some(agent.id.clone()),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::ReadOnly,
                priority: 0,
                required_capabilities: Vec::new(),
                max_attempts: 1,
                retry_backoff_seconds: 1,
            })
            .unwrap();
        let target_run = fixture
            .store
            .claim_task(ClaimTaskInput {
                task_id: task.id.clone(),
                agent_id: agent.id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store
            .update_run(UpdateRunInput {
                run_id: target_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store
            .finalize_successful_run(
                &target_run.id,
                &project.root_path,
                "初次完成",
                true,
                None,
                None,
            )
            .unwrap();

        let governance = CollaborationGovernance::new(fixture.store.clone());
        assert_eq!(
            governance.evaluate_quiescence(&project.id).unwrap(),
            WatchdogDisposition::Scheduled
        );
        let snapshot = fixture.store.snapshot(&project.id).unwrap();
        let review_task = snapshot
            .tasks
            .iter()
            .find(|candidate| governance.is_watchdog_task(&candidate.id).unwrap())
            .unwrap();
        let reviewer_id = review_task.assignee_agent_id.clone().unwrap();
        let review_run = fixture
            .store
            .claim_task(ClaimTaskInput {
                task_id: review_task.id.clone(),
                agent_id: reviewer_id,
                lease_seconds: 60,
            })
            .unwrap();
        fixture
            .store
            .update_run(UpdateRunInput {
                run_id: review_run.id.clone(),
                status: RunStatus::Running,
                output_summary: None,
                error: None,
                session_locator: None,
            })
            .unwrap();
        fixture
            .store
            .submit_review_decision(SubmitReviewDecisionInput {
                review_run_id: review_run.id.clone(),
                target_task_id: task.id.clone(),
                verdict: ReviewVerdict::ChangesRequested,
                summary: "发现遗漏，需要补充".to_string(),
            })
            .unwrap();
        fixture
            .store
            .finalize_successful_run(
                &review_run.id,
                &project.root_path,
                "语义复核退回",
                true,
                None,
                None,
            )
            .unwrap();

        let snapshot = fixture.store.snapshot(&project.id).unwrap();
        let target = snapshot
            .tasks
            .iter()
            .find(|candidate| candidate.id == task.id)
            .unwrap();
        assert_eq!(target.status, TaskStatus::ChangesRequested);
        assert_eq!(target.max_attempts, 2);
        assert_eq!(
            snapshot
                .tasks
                .iter()
                .find(|candidate| candidate.id == review_task.id)
                .unwrap()
                .status,
            TaskStatus::Done
        );
        let connection = fixture.store.connection.lock().unwrap();
        let watchdog_status: String = connection
            .query_row(
                "SELECT status FROM ma_semantic_watchdogs WHERE project_id = ?1",
                params![project.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(watchdog_status, "issues_found");
    }
}
