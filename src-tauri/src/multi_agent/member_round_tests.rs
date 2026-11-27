use super::*;
use crate::multi_agent::member_messages::{MemberSend, Purpose, TurnLimit};
use crate::sessions::{NewAgentSession, SessionManager};

#[test]
fn mention_direct_message_and_design_delegation_share_the_origin_and_leave_new_user_requests_free(
) -> Result<()> {
    let fixture = Fixture::new();
    fixture.git();
    let store = &fixture.store;
    let (a, b) = (&fixture.agents[0], &fixture.agents[1]);
    let mut input = fixture.input();
    input.native_capabilities = true;
    input.intent = "exchange".into();
    input.max_rounds = 2;
    let control = |text: &str, footer: Value| {
        format!("{text}{TURN_CONTROL_OPEN}{footer}{TURN_CONTROL_CLOSE}")
    };
    let replies = ScriptedReplies::new(vec![
        control(
            "需要接收方检查文件",
            json!({"action":"continue","nextAgentId":b,"hasNewInformation":true}),
        ),
        control(
            "已发现遗漏的检查项",
            json!({"action":"continue","nextAgentId":a,"hasNewInformation":true}),
        ),
        control("已登记待补事项", json!({"action":"done"})),
    ]);
    let snapshot = fixture.run_input(input, &replies);
    let round = &snapshot.rounds[0];
    assert_eq!(round.status, "completed", "{:?}", round.stop_reason);
    assert_eq!(replies.calls.lock().unwrap().len(), 3);
    let draft = fixture.draft();
    let plan = store.save_conversation_plan(
        &round.id,
        &snapshot.messages.last().unwrap().id,
        &json!({"summary":draft.summary,"tasks":draft.tasks}).to_string(),
    )?;
    let tasks = store.apply_conversation_plan(&ConversationExecutePlanInput {
        project_id: fixture.project.clone(),
        plan_id: plan.id,
        client_request_id: "source-budget".into(),
    })?;
    let run = store.claim_task(ClaimTaskInput {
        task_id: tasks[0].clone(),
        agent_id: a.clone(),
        lease_seconds: 120,
    })?;
    store.update_run(UpdateRunInput {
        run_id: run.id.clone(),
        status: RunStatus::Running,
        output_summary: None,
        error: None,
        session_locator: None,
    })?;
    let sessions = SessionManager::open_at(
        fixture.root.join("sessions.json"),
        fixture.root.join("events"),
    )?;
    let session = |agent: &str| -> Result<_> {
        let s = sessions.create_agent(NewAgentSession {
            project_id: fixture.project.clone(),
            agent_id: agent.to_string(),
            agent_name: agent.into(),
            runtime_kind: "codex".into(),
            avatar_id: None,
            cwd: fixture.root.display().to_string(),
        })?;
        store.link_agent_app_session(&fixture.project, agent, &s.id)?;
        Ok(s)
    };
    let (sender, target) = (session(a)?, session(b)?);
    assert!(store.record_bound_session_message(
        &sender,
        &target,
        "legacy-outbox-source",
        "请核实旧客户端的问题",
        run.created_at + 10
    )?);
    assert!(store.record_bound_session_message(
        &sender,
        &target,
        "legacy-outbox-source",
        "请核实旧客户端的问题",
        run.created_at + 10
    )?);
    let outsider = store.create_agent(CreateAgentInput {
        project_id: fixture.project.clone(),
        name: "旁观".into(),
        avatar_id: None,
        runtime_kind: AgentRuntimeKind::Codex,
        role: AgentRole::Worker,
        description: String::new(),
        model: None,
        permission_profile: PermissionProfile::Default,
        max_concurrency: 1,
        persona: Default::default(),
        persona_custom: String::new(),
    })?;
    let send = |purpose, to: &str| MemberSend {
        body: "核验结果".into(),
        target_agent_id: Some(to.into()),
        purpose: Some(purpose),
        handoff: None,
        request_id: None,
    };
    assert!(store
        .send_member_message(
            &fixture.project,
            &tasks[0],
            &run.id,
            a,
            send(Purpose::Question, &outsider.id),
            false
        )
        .is_err());
    let delegation = || {
        serde_json::from_value(json!({"targetAgentId":b,"title":"设计补充","brief":"定位遗漏","criteria":["留下证据"],"checkpoint":"当前工作已保存"})).unwrap()
    };
    let id = store.request_design(&run.id, a, delegation())?;
    assert_eq!(id, store.request_design(&run.id, a, delegation())?);
    let error = store
        .send_member_message(
            &fixture.project,
            &tasks[0],
            &run.id,
            a,
            send(Purpose::Question, b),
            false,
        )
        .unwrap_err();
    assert!(error.is::<TurnLimit>(), "{error:#}");
    let status = store.send_member_message(
        &fixture.project,
        &tasks[0],
        &run.id,
        a,
        send(Purpose::Status, b),
        false,
    )?;
    assert_eq!(status["sourceId"], round.user_message_id);
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db"))?;
    assert!(reopened
        .send_member_message(
            &fixture.project,
            &tasks[0],
            &run.id,
            a,
            send(Purpose::Question, b),
            false
        )
        .unwrap_err()
        .is::<TurnLimit>());
    let unrelated = store.create_task(CreateTaskInput {
        project_id: fixture.project.clone(),
        parent_id: None,
        stage: 0,
        title: "其他工作".into(),
        description: String::new(),
        acceptance_criteria: "独立验收".into(),
        verification_command: None,
        assignee_agent_id: Some(b.clone()),
        reviewer_agent_id: None,
        workspace_mode: WorkspaceMode::ReadOnly,
        priority: 0,
        required_capabilities: Vec::new(),
        max_attempts: 1,
        retry_backoff_seconds: 5,
    })?;
    assert!(!store
        .scoped_messages(&fixture.project, &unrelated.id, b, 200)?
        .iter()
        .any(|m| m.id == "legacy-outbox-source"));
    let mut next = fixture.input();
    next.native_capabilities = true;
    next.client_message_id = "user-new-source".into();
    let (next, _) = store.begin_conversation(next)?;
    let turn = prepare_turn(store, &next.id, 0, "chat", 0, b)?;
    let next_run = store.claim_task(ClaimTaskInput {
        task_id: turn.task_id.clone(),
        agent_id: b.clone(),
        lease_seconds: 120,
    })?;
    let next_message = store.send_member_message(
        &fixture.project,
        &turn.task_id,
        &next_run.id,
        b,
        send(Purpose::Question, a),
        false,
    )?;
    assert_ne!(next_message["sourceId"], status["sourceId"]);
    Ok(())
}

#[test]
fn late_legacy_outbox_is_bound_to_its_publishing_run_and_ambiguous_delivery_is_rejected(
) -> Result<()> {
    use crate::control::{MessageOrigin, OutboxMessage, SessionActionQueue};
    let f = Fixture::new();
    let s = &f.store;
    let mut input = f.input();
    input.native_capabilities = true;
    let (round, _) = s.begin_conversation(input)?;
    let turn = prepare_turn(s, &round.id, 0, "chat", 0, &f.agents[0])?;
    let run = s.claim_task(ClaimTaskInput {
        task_id: turn.task_id.clone(),
        agent_id: f.agents[0].clone(),
        lease_seconds: 120,
    })?;
    s.update_run(UpdateRunInput {
        run_id: run.id.clone(),
        status: RunStatus::Running,
        error: None,
        output_summary: None,
        session_locator: None,
    })?;
    let sessions = SessionManager::open_at(f.root.join("sessions.json"), f.root.join("events"))?;
    let mut members = Vec::new();
    for agent in &f.agents {
        let member = sessions.create_agent(NewAgentSession {
            project_id: f.project.clone(),
            agent_id: agent.clone(),
            agent_name: agent.clone(),
            runtime_kind: "codex".into(),
            avatar_id: None,
            cwd: f.root.display().to_string(),
        })?;
        s.link_agent_app_session(&f.project, agent, &member.id)?;
        members.push(member);
    }
    let outbox = f.root.join("outbox");
    let queue = SessionActionQueue::open_at(f.root.join("mailbox.json"), outbox.clone())?;
    crate::messaging::write_outbox(
        &outbox,
        &OutboxMessage {
            to: members[1].id.clone(),
            from: Some(MessageOrigin {
                session_id: members[0].id.clone(),
                label: "发起人".into(),
            }),
            body: "较早的提问".into(),
        },
    )?;
    s.update_run(UpdateRunInput {
        run_id: run.id.clone(),
        status: RunStatus::Succeeded,
        output_summary: Some("结束".into()),
        error: None,
        session_locator: None,
    })?;
    let mut stamp = None;
    assert_eq!(
        queue.ingest_outbox_with_timestamp(&sessions, |message, id, at| {
            stamp = Some(at);
            assert!(s.record_bound_session_message(
                &members[0],
                &members[1],
                id,
                &message.body,
                at
            )?);
            Ok(())
        })?,
        1
    );
    let msg = queue.peek_next(&members[1].id).unwrap();
    let record = s
        .snapshot(&f.project)?
        .messages
        .into_iter()
        .find(|m| m.id == msg.id)
        .unwrap();
    assert_eq!(record.task_id.as_deref(), Some(turn.task_id.as_str()));
    // Two historical intervals covering the same file timestamp cannot establish
    // which task authorized a legacy envelope. No duplicate wakeup is admitted.
    let published = stamp.unwrap();
    s.connection.lock().unwrap().execute("INSERT INTO ma_runs(id,project_id,task_id,agent_id,attempt,status,created_at,finished_at,lease_expires_at)
        VALUES('overlap',?1,?2,?3,2,'succeeded',?4,?4,?4)", params![f.project,turn.task_id,f.agents[0],published])?;
    let rejected = s
        .record_bound_session_message(&members[0], &members[1], "ambiguous", "未知来源", published)
        .unwrap_err();
    assert!(crate::multi_agent::member_messages::permanent_rejection(
        &rejected
    ));
    assert!(!s
        .snapshot(&f.project)?
        .messages
        .iter()
        .any(|m| m.id == "ambiguous"));
    Ok(())
}
