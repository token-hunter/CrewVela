//! Durable execution receipts. Dispatching is distinct from CLI acceptance and task completion.
use super::*;

impl AutomationManager {
    pub(super) fn report_legacy_result(
        &self,
        id: &str,
        ok: bool,
        detail: String,
        now: u64,
    ) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let Some(item) = next.iter_mut().find(|a| a.id == id) else {
            return Ok(());
        };
        let mut run = new_run(now, "legacy", None);
        run.ok = ok;
        run.stage = Some(if ok { "completed" } else { "failed" }.into());
        run.detail = detail;
        item.recent_runs.push(run);
        trim_history(item);
        settle_failure(item, ok);
        self.persist(&next)?;
        *items = next;
        self.dispatching.lock().unwrap().remove(id);
        Ok(())
    }

    /// A duplicate request returns its receipt and never dispatches again, even after history trimming.
    pub fn begin_manual(
        &self,
        id: &str,
        request_id: &str,
        retry_of: Option<&str>,
        now: u64,
    ) -> Result<Option<(String, AutomationAction)>> {
        uuid::Uuid::parse_str(request_id).context("执行请求编号无效")?;
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let item = next.iter_mut().find(|a| a.id == id).context("计划不存在")?;
        if item.request_receipts.contains_key(request_id) {
            return Ok(None);
        }
        if item.request_receipts.len() >= 4096 {
            anyhow::bail!("本计划的手动请求已达上限，请创建新计划；旧请求不会重放");
        }
        if item
            .recent_runs
            .iter()
            .any(|r| is_active_stage(r.stage.as_deref().unwrap_or("unconfirmed")))
        {
            anyhow::bail!("上一轮仍在执行或结果未确认，请先核对该轮，不能重复派发");
        }
        if let Some(previous) = retry_of {
            let run = item
                .recent_runs
                .iter()
                .find(|r| r.id == previous)
                .context("重试记录不存在")?;
            if !matches!(run.stage.as_deref(), Some("failed" | "cancelled")) {
                anyhow::bail!("只能重试已确认失败或取消的执行");
            }
        }
        let run = new_run(
            now,
            if retry_of.is_some() {
                "retry"
            } else {
                "manual"
            },
            retry_of.map(str::to_owned),
        );
        let result = (run.id.clone(), item.action.clone());
        item.request_receipts
            .insert(request_id.into(), run.id.clone());
        item.recent_runs.push(run);
        trim_history(item);
        self.persist(&next)?;
        *items = next;
        self.dispatching.lock().unwrap().insert(id.into());
        // Manual execution does not consume schedule limits or change enabled/next_run_at.
        Ok(Some(result))
    }

    /// Persist target identity before its message can enter the delivery queue.
    pub fn register_target(&self, run_id: &str, target: AutomationTarget) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let run = next
            .iter_mut()
            .flat_map(|a| a.recent_runs.iter_mut())
            .find(|r| r.id == run_id)
            .context("执行记录不存在")?;
        if run.stage.as_deref() != Some("dispatching") {
            anyhow::bail!("执行已不再允许派发");
        }
        run.targets.push(target);
        run.updated_at = now_millis();
        self.persist(&next)?;
        *items = next;
        Ok(())
    }

    pub fn finish_dispatch(
        &self,
        id: &str,
        run_id: &str,
        ok: bool,
        detail: String,
        now: u64,
    ) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let item = next.iter_mut().find(|a| a.id == id).context("计划不存在")?;
        let run = item
            .recent_runs
            .iter_mut()
            .find(|r| r.id == run_id)
            .context("执行记录不存在")?;
        if run.stage.as_deref() != Some("dispatching") {
            anyhow::bail!("执行已结算派发，拒绝重复写入");
        }
        // A failed enqueue is safe to retry only when NO target might have received a message.
        // The dispatcher registers all target identities before the atomic queue write.
        run.stage = Some(if ok { "queued" } else { "failed" }.into());
        run.ok = ok;
        run.detail = detail;
        run.updated_at = now;
        if !ok {
            for target in &mut run.targets {
                target.stage = "failed".into();
            }
        }
        item.active_session_ids = run.targets.iter().map(|t| t.session_id.clone()).collect();
        if !ok {
            settle_failure(item, false);
        }
        let saved = self.persist(&next);
        self.dispatching.lock().unwrap().remove(id);
        saved?;
        *items = next;
        Ok(())
    }

    /// Only exact message/task/run evidence may advance an execution. A ready terminal is not evidence.
    pub fn reconcile(
        &self,
        queue: &crate::control::SessionActionQueue,
        sessions: &crate::sessions::SessionManager,
        store: &crate::multi_agent::MultiAgentStore,
        now: u64,
    ) -> Result<()> {
        let evidence = queue.communication_evidence();
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let mut changed = false;
        for item in &mut next {
            let mut settlements = Vec::new();
            for run in &mut item.recent_runs {
                if !is_active_stage(run.stage.as_deref().unwrap_or("unconfirmed"))
                    || (run.stage.as_deref() == Some("dispatching")
                        && self.dispatching.lock().unwrap().contains(&item.id))
                {
                    continue;
                }
                if run.targets.is_empty() {
                    if run.stage.as_deref() != Some("unconfirmed") {
                        run.stage = Some("unconfirmed".into());
                        run.detail = "派发结果未能落盘且缺少目标回执，请核对；不会自动重发".into();
                        run.updated_at = now;
                        changed = true;
                    }
                    continue;
                }
                let before = serde_json::to_value(&*run)?;
                for target in &mut run.targets {
                    if matches!(target.stage.as_str(), "completed" | "failed" | "cancelled") {
                        continue;
                    }
                    let snapshot = match target
                        .project_id
                        .as_ref()
                        .map(|project| store.snapshot(project))
                        .transpose()
                    {
                        Ok(snapshot) => snapshot,
                        Err(error) => {
                            target.stage = "unconfirmed".into();
                            target.detail = format!("读取原项目回执失败：{error:#}");
                            continue;
                        }
                    };
                    let task = snapshot.and_then(|snapshot| {
                        let task = snapshot
                            .tasks
                            .iter()
                            .find(|t| Some(&t.id) == target.task_id.as_ref())?
                            .clone();
                        let latest = snapshot
                            .runs
                            .iter()
                            .filter(|r| r.task_id == task.id)
                            .max_by_key(|r| (r.attempt, r.created_at))
                            .cloned();
                        Some((task, latest))
                    });
                    if let Some((task, latest)) = task {
                        use crate::multi_agent::{RunStatus, TaskStatus};
                        target.run_id = latest.as_ref().map(|r| r.id.clone());
                        target.stage = match task.status {
                            TaskStatus::Done => "completed",
                            TaskStatus::Cancelled => "cancelled",
                            TaskStatus::Failed => "failed",
                            TaskStatus::WaitingApproval | TaskStatus::InReview => "waiting",
                            _ => {
                                if latest.as_ref().is_some_and(|r| {
                                    matches!(
                                        r.status,
                                        RunStatus::WaitingInput | RunStatus::WaitingApproval
                                    )
                                }) {
                                    "waiting"
                                } else if latest.as_ref().is_some_and(|r| {
                                    matches!(r.status, RunStatus::Running | RunStatus::Starting)
                                }) {
                                    "running"
                                } else {
                                    "queued"
                                }
                            }
                        }
                        .into();
                        if latest.as_ref().is_some_and(|r| {
                            r.session_locator
                                .as_ref()
                                .is_some_and(|l| l["stopUnverified"] == true)
                        }) {
                            target.stage = "unconfirmed".into();
                        }
                        target.detail = latest
                            .and_then(|r| r.error.or(r.output_summary))
                            .unwrap_or_else(|| format!("项目任务：{:?}", task.status));
                    } else if let Some((stage, detail)) =
                        sessions.automation_message_evidence(&target.message_id)
                    {
                        target.stage = stage;
                        target.detail = detail;
                    } else if let Some(event) = evidence.iter().rev().find(|e| {
                        e["id"].as_str() == Some(&target.message_id)
                            && e["toSessionId"].as_str() == Some(&target.session_id)
                    }) {
                        target.stage = match event["stage"].as_str() {
                            Some("queued") => "queued",
                            Some("submit_sent" | "text_written") => "submitted",
                            Some("bridge_received") => "received",
                            _ => "unconfirmed",
                        }
                        .into();
                        target.detail = if event["held"].as_bool() == Some(true) {
                            target.stage = "unconfirmed".into();
                            "重启后暂停重放，请核对原会话"
                        } else if target.stage == "queued" {
                            "消息仍在队列中"
                        } else {
                            "已观察到投递；缺少与本消息关联的执行结束回执"
                        }
                        .into();
                    } else {
                        target.stage = "unconfirmed".into();
                        target.detail = "缺少本消息的可靠回执，请检查原会话；不会自动重发".into();
                    }
                    if sessions.get(&target.session_id).is_none() && target.stage != "completed" {
                        target.stage = "unconfirmed".into();
                        target.detail = "原会话已不存在，结果未确认".into();
                    }
                    if matches!(target.stage.as_str(), "submitted" | "received")
                        && now.saturating_sub(run.at) >= 60_000
                    {
                        target.stage = "unconfirmed".into();
                        target.detail =
                            "已观察到投递，但尚无可关联的执行结果回传；请核对原会话，不会自动重发"
                                .into();
                    }
                    if matches!(target.stage.as_str(), "running" | "received" | "submitted")
                        && sessions.get(&target.session_id).is_some_and(|s| {
                            matches!(
                                s.status,
                                crate::sessions::SessionStatus::Stopped
                                    | crate::sessions::SessionStatus::Error
                            )
                        })
                    {
                        target.stage = "unconfirmed".into();
                        target.detail = "原 CLI 已停止或异常，但没有与本消息关联的结束回执".into();
                    }
                    if matches!(
                        target.stage.as_str(),
                        "queued" | "submitted" | "received" | "running"
                    ) && sessions
                        .get(&target.session_id)
                        .is_some_and(|s| s.status == crate::sessions::SessionStatus::Waiting)
                    {
                        target.stage = "waiting".into();
                        target.detail =
                            "目标会话正在等待人工处理，请打开原会话；不会自动确认权限".into();
                    }
                }
                let stages: Vec<_> = run.targets.iter().map(|t| t.stage.as_str()).collect();
                let stage = if stages
                    .iter()
                    .all(|s| matches!(*s, "completed" | "failed" | "cancelled"))
                {
                    if stages.contains(&"failed") {
                        "failed"
                    } else if stages.contains(&"cancelled") {
                        "cancelled"
                    } else {
                        "completed"
                    }
                } else if stages.contains(&"unconfirmed") {
                    "unconfirmed"
                } else if stages.contains(&"waiting") {
                    "waiting"
                } else if stages.contains(&"running") {
                    "running"
                } else if stages.contains(&"received") {
                    "received"
                } else if stages.contains(&"submitted") {
                    "submitted"
                } else {
                    "queued"
                };
                let stage = if run.stage.as_deref() == Some("stopping")
                    && !matches!(stage, "completed" | "failed" | "cancelled" | "unconfirmed")
                {
                    "stopping"
                } else {
                    stage
                };
                run.stage = Some(stage.into());
                run.ok = !matches!(stage, "failed" | "cancelled");
                run.detail = if run.targets.len() == 1 {
                    run.targets[0].detail.clone()
                } else {
                    format!(
                        "{} 个目标；{} 个已结束。逐项回执见详情",
                        stages.len(),
                        stages
                            .iter()
                            .filter(|s| matches!(**s, "completed" | "failed" | "cancelled"))
                            .count()
                    )
                };
                if before != serde_json::to_value(&*run)? {
                    run.updated_at = now;
                    changed = true;
                    if matches!(stage, "completed" | "failed") {
                        settlements.push(stage == "completed");
                    }
                }
            }
            for ok in settlements {
                settle_failure(item, ok);
            }
            trim_history(item);
        }
        if changed {
            self.persist(&next)?;
            *items = next;
        }
        Ok(())
    }

    pub fn cancel_queued_run(
        &self,
        id: &str,
        run_id: &str,
        queue: &crate::control::SessionActionQueue,
    ) -> Result<()> {
        let run = self
            .list()
            .into_iter()
            .find(|a| a.id == id)
            .and_then(|a| a.recent_runs.into_iter().find(|r| r.id == run_id))
            .context("执行不存在")?;
        if run.targets.is_empty() {
            anyhow::bail!("本轮尚在派发或缺少身份，不能安全停止");
        }
        // Queue owns the delivery mutex. It atomically refuses the WHOLE batch if any target was already written/accepted.
        queue.cancel_automation_messages(
            &run.targets
                .iter()
                .map(|t| (t.session_id.clone(), t.message_id.clone()))
                .collect::<Vec<_>>(),
        )?;
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let run = next
            .iter_mut()
            .find(|a| a.id == id)
            .and_then(|a| a.recent_runs.iter_mut().find(|r| r.id == run_id))
            .context("执行不存在")?;
        run.stage = Some("cancelled".into());
        run.detail = "本轮未投递的消息已撤销；后续计划保持原状态".into();
        run.updated_at = now_millis();
        for target in &mut run.targets {
            target.stage = "cancelled".into();
        }
        self.persist(&next)?;
        *items = next;
        Ok(())
    }

    pub fn mark_stopping(&self, id: &str, run_id: &str) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let run = next
            .iter_mut()
            .find(|a| a.id == id)
            .and_then(|a| a.recent_runs.iter_mut().find(|r| r.id == run_id))
            .context("执行不存在")?;
        run.stage = Some("stopping".into());
        run.detail = "已请求停止本轮具体 Run，等待退出确认；未暂停后续计划".into();
        run.updated_at = now_millis();
        self.persist(&next)?;
        *items = next;
        Ok(())
    }

    pub fn mark_cancelled(&self, id: &str, run_id: &str) -> Result<()> {
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let run = next
            .iter_mut()
            .find(|a| a.id == id)
            .and_then(|a| a.recent_runs.iter_mut().find(|r| r.id == run_id))
            .context("执行不存在")?;
        run.stage = Some("cancelled".into());
        run.detail = "本轮尚未启动的项目任务已取消；后续计划保持原状态".into();
        run.updated_at = now_millis();
        for target in &mut run.targets {
            target.stage = "cancelled".into();
        }
        self.persist(&next)?;
        *items = next;
        Ok(())
    }

    /// Explicit human acknowledgement releases uncertainty without claiming success or replaying mail.
    pub fn acknowledge_unknown(
        &self,
        id: &str,
        run_id: &str,
        queue: &crate::control::SessionActionQueue,
        store: &crate::multi_agent::MultiAgentStore,
    ) -> Result<()> {
        let run = self
            .list()
            .into_iter()
            .find(|a| a.id == id)
            .and_then(|a| a.recent_runs.into_iter().find(|r| r.id == run_id))
            .context("执行不存在")?;
        if run.stage.as_deref() != Some("unconfirmed") {
            anyhow::bail!("仅结果未确认的记录可以人工核对");
        }
        for target in &run.targets {
            // Remove any still-unsent exact mail; written/accepted mail cannot be removed here.
            if queue.contains_message(&target.session_id, &target.message_id) {
                queue.cancel_automation_messages(&[(
                    target.session_id.clone(),
                    target.message_id.clone(),
                )])?;
            }
            if let Some(project) = &target.project_id {
                let snapshot = store.snapshot(project)?;
                if snapshot.tasks.iter().any(|t| {
                    Some(&t.id) == target.task_id.as_ref()
                        && !matches!(
                            t.status,
                            crate::multi_agent::TaskStatus::Done
                                | crate::multi_agent::TaskStatus::Failed
                                | crate::multi_agent::TaskStatus::Cancelled
                        )
                }) {
                    anyhow::bail!("仍有活动项目任务，请先停止本轮并确认原进程退出");
                }
            }
        }
        let mut items = self.items.lock().unwrap();
        let mut next = items.clone();
        let current = next
            .iter_mut()
            .find(|a| a.id == id)
            .and_then(|a| a.recent_runs.iter_mut().find(|r| r.id == run_id))
            .context("执行不存在")?;
        if current.stage.as_deref() != Some("unconfirmed") {
            anyhow::bail!("状态已变化，请刷新后重新核对");
        }
        current.stage = Some("acknowledged".into());
        current.detail = "用户已核对原会话；实际结果仍未知，未计为成功，也未重发消息".into();
        current.updated_at = now_millis();
        self.persist(&next)?;
        *items = next;
        Ok(())
    }
}
