use super::*;
use crate::codex_storage::{collaboration_ready, LOGIN_REQUIRED};
use std::path::Path;

pub(super) const LOGIN_REPLY: &str = "codex_login_required";

// A failed, unopened collaboration session is safe to resubmit. An existing
// native thread or uncertain process needs its normal resume/recovery flow.
pub(super) fn unopened_login_failure(error: Option<&str>, locator: Option<&str>) -> bool {
    let Some(error) = error else {
        return false;
    };
    let Some(value) = locator.and_then(|raw| serde_json::from_str::<Value>(raw).ok()) else {
        return false;
    };
    value["codexStorage"] == "crewvela"
        && value["stopUnverified"] != true
        && ["sessionId", "threadId", "thread_id", "managedTurn"]
            .iter()
            .all(|key| value[*key].is_null())
        && error.contains("Codex 专用目录认证失败")
        && error.contains("Authentication required")
}

pub(super) fn block_before_dispatch(
    store: &MultiAgentStore,
    turn: &Turn,
    error: Option<&str>,
) -> Result<()> {
    let mut connection = store.connection.lock().unwrap();
    let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
    ensure!(
        tx.execute(
            "UPDATE ma_tasks SET status='failed',updated_at=?2 WHERE id=?1 AND status='ready'",
            params![turn.task_id, now()]
        )? == 1,
        "成员状态已变化，未派单"
    );
    let mut reply = message_with(&tx, &turn.id)?;
    reply.status = "failed".into();
    reply.updated_at = now();
    if error.is_none() {
        reply.kind = LOGIN_REPLY.into();
    }
    save_message(&tx, &format!("turn:{}", turn.id), &reply)?;
    if let Some(reason) = error {
        reply.id = format!("status-{}", turn.id);
        reply.sender_type = "system".into();
        reply.agent_id = None;
        reply.kind = "status".into();
        reply.body = reason.into();
        save_message(&tx, &format!("run-error:{}", turn.id), &reply)?;
    }
    tx.commit()?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexRetryInput {
    pub project_id: String,
    pub expected_conversation_id: String,
    pub round_id: String,
    pub turns: Vec<CodexFailedTurn>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CodexFailedTurn {
    pub message_id: String,
    pub run_id: Option<String>,
}

impl MultiAgentStore {
    pub(crate) fn plan_auth_preflight(
        &self,
        input: &ConversationSendInput,
    ) -> Result<Option<ConversationPlanIssue>> {
        if input.mode != "execution_plan" {
            return Ok(None);
        }
        if !input.managed && !input.native_capabilities {
            return Ok(None);
        }
        // Manual proposals only dispatch their first member. Managed execution
        // is already authorized, including the selected development/review team.
        let members =
            input
                .participant_agent_ids
                .iter()
                .take(if input.managed { usize::MAX } else { 1 });
        let ready = self.codex_members_ready(&input.project_id, members, !input.managed)?;
        Ok((!ready).then(|| ConversationPlanIssue {
            kind: "codex_login",
            message: LOGIN_REQUIRED.into(),
        }))
    }

    fn codex_members_ready<'a>(
        &self,
        project: &str,
        members: impl Iterator<Item = &'a String>,
        acp: bool,
    ) -> Result<bool> {
        for member in members {
            let profile = self.agent_profile(member)?;
            ensure!(profile.project_id == project, "成员不属于本项目");
            if profile.runtime_kind == AgentRuntimeKind::Codex {
                return collaboration_ready(Path::new(&self.project(project)?.root_path), acp);
            }
        }
        Ok(true)
    }

    pub(crate) fn plan_accounts_ready(&self, input: &ConversationExecutePlanInput) -> Result<bool> {
        let raw = self.connection.lock().unwrap().query_row(
            "SELECT data_json FROM ma_conversation_plans WHERE id=?1 AND project_id=?2",
            params![input.plan_id, input.project_id],
            |row| row.get::<_, String>(0),
        )?;
        let plan: ConversationExecutionPlan = decode(raw)?;
        if plan.status == "pending" {
            let members = plan.tasks.iter().flat_map(|task| {
                std::iter::once(&task.assignee_agent_id).chain(task.reviewer_agent_id.iter())
            });
            // Explicit launch modes, mixed CLI teams and communication contracts
            // use the native CLI. ACP-only default credentials do not apply there.
            let mut acp = plan.managed.is_none();
            if acp && std::env::var_os("DEFAULT_AUTH_REQUEST").is_some() {
                let project = self.snapshot(&input.project_id)?;
                let single_runtime = project
                    .agents
                    .iter()
                    .filter(|a| a.enabled)
                    .all(|a| a.runtime_kind == AgentRuntimeKind::Codex);
                let saved = crate::runtime_preferences::get()?
                    .default_launch_modes
                    .contains_key("codex");
                acp = single_runtime
                    && plan.tasks.iter().all(|task| {
                        (!saved || task.workspace_mode == WorkspaceMode::ReadOnly)
                            && !super::super::contract_requires_session_cli(
                                &task.description,
                                &task.acceptance_criteria,
                            )
                    });
            }
            return self.codex_members_ready(&input.project_id, members, acp);
        }
        Ok(true)
    }

    pub(crate) fn resume_managed_after_login(
        &self,
        project: &str,
        conversation: &str,
        plan: &str,
        engineering: &super::super::MultiAgentSupervisor,
        host: &AgentTerminalHost,
        bridge: &CollaborationBridge,
    ) -> Result<()> {
        let round_id = {
            let connection = self.connection.lock().unwrap();
            ensure_active_conversation(
                Some(conversation),
                &conversation_with(&connection, project)?,
            )?;
            let raw = connection.query_row(
                "SELECT data_json FROM ma_conversation_plans WHERE id=?1 AND project_id=?2",
                params![plan, project],
                |row| row.get::<_, String>(0),
            )?;
            let plan: ConversationExecutionPlan = decode(raw)?;
            let round = round_with(&connection, &plan.round_id)?;
            ensure!(
                round.conversation_id == conversation && round.managed_target.is_some(),
                "托管轮次不属于当前对话"
            );
            if plan.status == "applied" {
                return Ok(());
            }
            ensure!(
                plan.status == "pending"
                    && plan.managed.is_some_and(|m| m.status == "waiting_login"),
                "托管分工没有等待登录"
            );
            round.id
        };
        self.start_managed_round(&round_id, engineering, host, bridge)
    }

    pub(super) fn prepare_codex_retries(
        &self,
        input: &CodexRetryInput,
    ) -> Result<Option<(ConversationRound, Vec<Turn>)>> {
        let mut connection = self.connection.lock().unwrap();
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        ensure_active_conversation(
            Some(&input.expected_conversation_id),
            &conversation_with(&tx, &input.project_id)?,
        )?;
        let mut round = round_with(&tx, &input.round_id)?;
        ensure!(
            round.project_id == input.project_id
                && round.conversation_id == input.expected_conversation_id
                && round.mode == "discussion",
            "只能补发当前群聊中登录失败的成员"
        );
        ensure!(
            !input.turns.is_empty()
                && input.turns.len() <= round.participant_agent_ids.len().max(24),
            "无效的重试成员列表"
        );
        sync_project_messages(&tx, &input.project_id)?;
        let mut turns = Vec::new();
        let mut seen = HashSet::new();
        for source in &input.turns {
            if !seen.insert(&source.message_id) {
                continue;
            }
            let message = message_with(&tx, &source.message_id)?;
            ensure!(
                message.round_id.as_ref() == Some(&round.id) && message.sender_type == "agent",
                "无效的重试消息"
            );
            if message.kind != LOGIN_REPLY
                || message.status != "failed"
                || message.run_id != source.run_id
            {
                continue;
            }
            let turn: Turn = decode(tx.query_row(
                "SELECT data_json FROM ma_conversation_turns WHERE id=?1 AND round_id=?2",
                params![message.id, round.id],
                |row| row.get(0),
            )?)?;
            let can_retry: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM ma_tasks t JOIN ma_agents a ON t.assignee_agent_id=a.id
                JOIN ma_projects p ON p.id=t.project_id WHERE t.id=?1 AND a.id=?2 AND a.project_id=?3 AND t.project_id=?3
                AND a.runtime_kind='codex' AND a.enabled=1 AND t.status IN ('failed','ready') AND p.status!='cancelled'
                AND NOT EXISTS(SELECT 1 FROM ma_agent_sessions s WHERE s.agent_id=a.id AND s.control_mode='manual')
                AND NOT EXISTS(SELECT 1 FROM ma_runs r WHERE r.task_id=t.id AND (r.status NOT IN ('failed','timed_out')
                    OR json_extract(r.session_locator_json,'$.stopUnverified')=1)))",
                params![turn.task_id, turn.agent_id, input.project_id], |row| row.get(0))?;
            ensure!(can_retry, "成员已停用、被接管或执行状态已变化，请重新读取");
            turns.push(turn);
        }
        if turns.is_empty() {
            return Ok(None);
        }
        ensure!(
            matches!(
                round.status.as_str(),
                "failed" | "needs_decision" | "completed"
            ),
            "请等待原讨论结束后重试成员"
        );
        ensure_conversation_can_change(&tx, &input.project_id, "重试失败成员")?;
        let request: ConversationSendInput = decode(tx.query_row(
            "SELECT request_json FROM ma_conversation_rounds WHERE id=?1",
            [&round.id],
            |row| row.get(0),
        )?)?;
        round.deadline_at = now() + i64::from(request.max_duration_seconds) * 1000;
        round.status = "queued".into();
        round.stop_reason = None;
        round.updated_at = now();
        save_round(&tx, &round)?;
        for turn in &turns {
            tx.execute(
                "UPDATE ma_tasks SET status='ready',updated_at=?2 WHERE id=?1",
                params![turn.task_id, now()],
            )?;
        }
        tx.commit()?;
        Ok(Some((round, turns)))
    }
}

impl ConversationSupervisor {
    pub(crate) fn retry_codex_members(
        &self,
        store: MultiAgentStore,
        input: CodexRetryInput,
        host: AgentTerminalHost,
        bridge: CollaborationBridge,
        engineering: super::super::MultiAgentSupervisor,
    ) -> Result<()> {
        let _lease = crate::runtime_preferences::acquire(AgentRuntimeKind::Codex)?;
        let mut active = self.active.lock().unwrap();
        ensure!(
            !active.contains_key(&input.project_id),
            "请等待当前讨论结束后重试"
        );
        if let Some((round, turns)) = store.prepare_codex_retries(&input)? {
            let flag = Arc::new(AtomicBool::new(false));
            active.insert(round.project_id.clone(), flag.clone());
            self.spawn_round(
                store,
                round,
                flag,
                host,
                bridge,
                Some(engineering),
                Some(turns),
            );
        }
        Ok(())
    }
}

pub(super) fn run_retries(
    store: &MultiAgentStore,
    round: &ConversationRound,
    turns: &[Turn],
    cancelled: &Arc<AtomicBool>,
    executor: &impl DiscussionExecutor,
) -> Result<()> {
    let _deadline = DeadlineGuard::start(
        store.clone(),
        round.clone(),
        cancelled.clone(),
        Arc::new(AtomicBool::new(false)),
    );
    let mut reasons = Vec::new();
    for turn in turns {
        if stop_if_requested(store, &round.id, cancelled)? {
            return Ok(());
        }
        let result = execute_discussion_turn(store, round, turn, executor, cancelled, || {
            discussion_stop_requested(store, &round.id, cancelled)
        })?;
        match parallel_reply_outcome(store, round, &turn.agent_id, result) {
            Ok(Some(("unconfirmed", _))) => {
                return store.finish_conversation(
                    &round.id,
                    "unconfirmed",
                    Some("成员停止未确认，请核对原进程".into()),
                )
            }
            Ok(Some((_, reason))) => reasons.push(reason),
            Err(error) => reasons.push(format!("{error:#}")),
            Ok(None) => {}
        }
    }
    if stop_if_requested(store, &round.id, cancelled)? {
        return Ok(());
    }
    let snapshot = store.conversation_snapshot(&round.project_id)?;
    let members = snapshot
        .messages
        .iter()
        .filter(|m| m.round_id.as_ref() == Some(&round.id) && m.sender_type == "agent");
    let failed = members.clone().any(|m| m.status == "failed");
    let waiting_for_human = {
        let connection = store.connection.lock().unwrap();
        let mut waiting = false;
        for member in members.clone().filter(|m| m.status == "completed") {
            let raw = message_with(&connection, &member.id)?;
            if raw.agent_id.as_ref().is_some_and(|id| {
                turn_control(&raw.body, round, id).is_ok_and(|control| {
                    control.is_some_and(|control| control.action == TurnAction::Wait)
                })
            }) {
                waiting = true;
                break;
            }
        }
        waiting
    };
    if waiting_for_human && reasons.is_empty() {
        reasons.push("有成员仍需要你补充必要信息".into());
    }
    let waiting = members.clone().any(|m| m.status != "completed") || !reasons.is_empty();
    let status = if failed {
        "failed"
    } else if waiting {
        "needs_decision"
    } else {
        "completed"
    };
    store.finish_conversation(
        &round.id,
        status,
        (!reasons.is_empty()).then(|| reasons.join("；")),
    )
}
