use super::*;
use crate::multi_agent::conversation_metrics::{codex_test_output, record_cli_usage};

fn report(fixture: &Fixture, round: &str) -> Value {
    serde_json::to_value(
        fixture
            .store
            .conversation_round_metrics(&fixture.project, round)
            .unwrap(),
    )
    .unwrap()
}

#[test]
fn same_long_log_late_review_and_observer_compare_dispatches_without_estimating_tokens() {
    fn run(parallel: bool) -> Value {
        let fixture = Fixture::new();
        let store = &fixture.store;
        let observer = store
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: fixture.project.clone(),
                name: "observer".into(),
                runtime_kind: AgentRuntimeKind::Codex,
                role: AgentRole::Reviewer,
                description: "只在被点名时审查".into(),
                persona: Default::default(),
                persona_custom: String::new(),
                model: None,
                permission_profile: PermissionProfile::Default,
                max_concurrency: 1,
            })
            .unwrap();
        let mut first = fixture.input();
        first.participant_agent_ids.truncate(1);
        first.body = "先记录部署输出，随后需要确定交付结论".into();
        let history = fixture.run_input(
            first,
            &ScriptedReplies::new(vec!["旧日志行 abc\n".repeat(2200)]),
        );
        let mut late = history
            .messages
            .iter()
            .find(|m| m.sender_type == "agent")
            .unwrap()
            .clone();
        late.id = id();
        late.agent_id = Some(fixture.agents[1].clone());
        late.task_id = None;
        late.run_id = None;
        late.status = "running".into();
        late.body.clear();
        save_message(
            &store.connection.lock().unwrap(),
            "fixture-late-review",
            &late,
        )
        .unwrap();

        let mut input = fixture.input();
        input.client_message_id = "same-verification-request".into();
        input.body = "依据刚到的审查结论，确认本轮发布条件。".into();
        input.participant_agent_ids.push(observer.id);
        input.chat_dispatch = Some(if parallel {
            ConversationChatDispatch::Parallel
        } else {
            ConversationChatDispatch::Assigned {
                agent_id: fixture.agents[0].clone(),
            }
        });
        let (round, _) = store.begin_conversation(input.clone()).unwrap();
        assert!(!store.begin_conversation(input).unwrap().1);
        late.body = "晚到审查结论：升级必须兼容 v3 持久化格式。".into();
        late.status = "completed".into();
        save_message(
            &store.connection.lock().unwrap(),
            "fixture-late-review",
            &late,
        )
        .unwrap();
        let executor = ScriptedReplies::new(vec!["收到，按 v3 校验。".into(); 3]);
        run_discussion(
            store,
            &round.id,
            &Arc::new(AtomicBool::new(false)),
            &executor,
        )
        .unwrap();
        let calls = executor.calls.lock().unwrap();
        for (_, prompt) in calls.iter() {
            assert!(
                prompt.contains(&late.id) && prompt.contains(&late.body),
                "晚到的已发布结论有可回溯的消息标识"
            );
            assert!(
                prompt.matches("旧日志行 abc").count() < 400,
                "完整长日志未再次注入"
            );
            assert_eq!(
                prompt
                    .matches("依据刚到的审查结论，确认本轮发布条件。")
                    .count(),
                1
            );
        }
        let before = report(&fixture, &round.id);
        assert_eq!(before["totals"]["starts"], calls.len() as u64);
        assert_eq!(before["totals"]["repeatDispatches"], 0);
        assert_eq!(
            before["totals"]["tokens"],
            json!({"inputTotal":null,"cacheRead":null,"output":null})
        );
        let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
        let after = serde_json::to_value(
            reopened
                .conversation_round_metrics(&fixture.project, &round.id)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(before, after);
        before
    }
    let explicit_fanout = run(true);
    let assigned = run(false);
    assert_eq!(explicit_fanout["totals"]["starts"], 3);
    assert_eq!(assigned["totals"]["starts"], 1);
    assert_eq!(
        assigned["unstartedMembers"],
        json!(["member-1", "observer"])
    );
}

#[test]
fn one_launch_with_many_receipts_is_not_a_repeat_and_missing_fields_stay_unknown() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    let (round, _) = store.begin_conversation(fixture.input()).unwrap();
    let turn = prepare_turn(store, &round.id, 0, "chat", 0, &fixture.agents[0]).unwrap();
    let run = store
        .claim_task(ClaimTaskInput {
            task_id: turn.task_id.clone(),
            agent_id: turn.agent_id.clone(),
            lease_seconds: 60,
        })
        .unwrap();
    assert_eq!(report(&fixture, &round.id)["totals"]["starts"], 0);
    store
        .update_run(UpdateRunInput {
            run_id: run.id.clone(),
            status: RunStatus::Running,
            output_summary: None,
            error: None,
            session_locator: None,
        })
        .unwrap();
    let uncertain = report(&fixture, &round.id);
    assert!(
        uncertain["totals"]["starts"].is_null(),
        "a scheduler transition is not a launch"
    );
    assert_eq!(
        uncertain["totals"]["repeatDispatches"], 0,
        "one possible run cannot be a repeat"
    );
    for _ in 0..2 {
        record_cli_usage(
            store,
            &run.id,
            AgentRuntimeKind::Codex,
            &codex_test_output(json!({"input_tokens":500,"cached_input_tokens":100})),
        );
    }
    let known = report(&fixture, &round.id);
    assert_eq!(
        known["totals"],
        json!({"starts":1,"repeatDispatches":0,"tokens":{"inputTotal":500,"cacheRead":100,"output":null}})
    );
    assert_eq!(known["tasks"][0]["metrics"], known["totals"]);
    assert!(store
        .conversation_round_metrics("other-project", &round.id)
        .is_err());
    store
        .update_run(UpdateRunInput {
            run_id: run.id,
            status: RunStatus::Failed,
            output_summary: None,
            error: Some("fixture".into()),
            session_locator: None,
        })
        .unwrap();
    // A second genuine attempt for the same causal message/member is a repeat.
    let repeated = prepare_turn(store, &round.id, 1, "chat", 0, &fixture.agents[0]).unwrap();
    let run = store
        .claim_task(ClaimTaskInput {
            task_id: repeated.task_id,
            agent_id: repeated.agent_id,
            lease_seconds: 60,
        })
        .unwrap();
    record_cli_usage(
        store,
        &run.id,
        AgentRuntimeKind::Codex,
        &codex_test_output(json!({"input_tokens":16,"cached_input_tokens":0,"output_tokens":3})),
    );
    let with_repeat = report(&fixture, &round.id);
    assert_eq!(
        with_repeat["totals"],
        json!({"starts":2,"repeatDispatches":1,"tokens":{"inputTotal":516,"cacheRead":100,"output":null}})
    );
}

#[test]
fn question_task_counts_in_its_originating_round_and_passive_status_creates_no_launch() {
    use crate::multi_agent::member_messages::{MemberSend, Purpose};
    let fixture = Fixture::new();
    let store = &fixture.store;
    let (round, _) = store.begin_conversation(fixture.input()).unwrap();
    let source = prepare_turn(store, &round.id, 0, "chat", 0, &fixture.agents[0]).unwrap();
    let run = store
        .claim_task(ClaimTaskInput {
            task_id: source.task_id.clone(),
            agent_id: source.agent_id.clone(),
            lease_seconds: 60,
        })
        .unwrap();
    store
        .append_runtime_event(crate::multi_agent::AppendRuntimeEventInput {
            run_id: run.id.clone(),
            event_type: "conversation_prompt_received".into(),
            payload: json!({}),
        })
        .unwrap();
    let send = |purpose, body: &str, request_id: &str| {
        store
            .send_member_message(
                &fixture.project,
                &source.task_id,
                &run.id,
                &source.agent_id,
                MemberSend {
                    purpose: Some(purpose),
                    body: body.into(),
                    target_agent_id: Some(fixture.agents[1].clone()),
                    handoff: None,
                    request_id: Some(request_id.into()),
                },
                false,
            )
            .unwrap()
    };
    let status_receipt = send(Purpose::Status, "私有状态正文", &id());
    let displayed_receipts = || {
        serde_json::to_value(store.conversation_snapshot(&fixture.project).unwrap()).unwrap()
            ["memberReceipts"][&round.user_message_id].clone()
    };
    assert_eq!(displayed_receipts()[0]["status"], "inbox");
    assert_eq!(displayed_receipts()[0]["messageId"], status_receipt["messageId"]);
    assert_eq!(displayed_receipts()[0]["arguments"]["body"], "私有状态正文");
    assert!(!store.conversation_snapshot(&fixture.project).unwrap().messages.iter().any(|message| message.body.contains("私有状态正文")));
    let queue = crate::control::SessionActionQueue::open_at(
        fixture.root.join("metrics-mailbox.json"),
        fixture.root.join("outbox"),
    )
    .unwrap();
    store.drain_member_inbox(&queue).unwrap();
    let status = report(&fixture, &round.id);
    assert_eq!(status["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(status["totals"]["starts"], 1);
    let request_id = id();
    let receipt = send(
        Purpose::Question,
        "私有问题正文不能出现在群聊统计中",
        &request_id,
    );
    assert_eq!(displayed_receipts()[1]["status"], "queued");
    assert_eq!(
        send(
            Purpose::Question,
            "私有问题正文不能出现在群聊统计中",
            &request_id
        )["messageId"],
        receipt["messageId"]
    );
    store.drain_member_inbox(&queue).unwrap();
    let child = format!("cli-message-{}", receipt["messageId"].as_str().unwrap());
    assert_eq!(
        report(&fixture, &round.id)["totals"]["starts"],
        1,
        "queued is not launched"
    );
    let run = store
        .claim_task(ClaimTaskInput {
            task_id: child.clone(),
            agent_id: fixture.agents[1].clone(),
            lease_seconds: 60,
        })
        .unwrap();
    assert_eq!(displayed_receipts()[1]["status"], "awaiting_receipt");
    assert_eq!(displayed_receipts()[1]["runId"], run.id);
    record_cli_usage(
        store,
        &run.id,
        AgentRuntimeKind::Codex,
        &codex_test_output(json!({"input_tokens":10,"cached_input_tokens":0,"output_tokens":4})),
    );
    let delivered = report(&fixture, &round.id);
    assert_eq!(delivered["totals"]["starts"], 2);
    assert_eq!(delivered["totals"]["repeatDispatches"], 0);
    assert_eq!(delivered["tasks"][1]["taskId"], child);
    assert_eq!(delivered["tasks"][1]["title"], "成员提问");
    assert!(!delivered.to_string().contains("私有问题正文"));
    store.update_run(UpdateRunInput { run_id: run.id.clone(), status: RunStatus::Running,
        output_summary: None, error: None, session_locator: None }).unwrap();
    store.update_run(UpdateRunInput { run_id: run.id, status: RunStatus::Succeeded,
        output_summary: Some("私有回答".into()), error: None, session_locator: None }).unwrap();
    assert_eq!(displayed_receipts()[1]["status"], "completed");
}
