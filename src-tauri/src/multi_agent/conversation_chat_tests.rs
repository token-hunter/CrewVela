use super::*;

#[test]
fn group_permission_resumes_only_its_waiting_run_and_never_starts_the_project() {
    let fixture = Fixture::new();
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    let turn = prepare_turn(&fixture.store, &round.id, 0, "chat", 0, &fixture.agents[0]).unwrap();
    let run = fixture.store.claim_task(ClaimTaskInput { task_id: turn.task_id.clone(), agent_id: fixture.agents[0].clone(), lease_seconds: 60 }).unwrap();
    let other = fixture.store.create_task(CreateTaskInput {
        project_id: fixture.project.clone(), parent_id: None, stage: 0, title: "未授权启动的工程任务".into(), description: String::new(), acceptance_criteria: "等待执行".into(), verification_command: None,
        assignee_agent_id: Some(fixture.agents[1].clone()), reviewer_agent_id: None, workspace_mode: WorkspaceMode::ReadOnly, priority: 0, required_capabilities: vec![], max_attempts: 1, retry_backoff_seconds: 1,
    }).unwrap();
    fixture.store.connection.lock().unwrap().execute("UPDATE ma_projects SET status='paused' WHERE id=?1", [&fixture.project]).unwrap();
    let approval = fixture.store.request_approval(crate::multi_agent::RequestApprovalInput { project_id: fixture.project.clone(), task_id: Some(turn.task_id.clone()), run_id: Some(run.id.clone()), approval_type: "runtime_permission".into(), summary: "确认当前命令".into() }).unwrap();
    let governance = crate::multi_agent::governance::CollaborationGovernance::new(fixture.store.clone());
    let item = governance.list_interventions().unwrap().into_iter().find(|item| item.metadata["approvalId"] == approval.id).unwrap();
    for _ in 0..2 {
        let receipt = governance.resolve(crate::multi_agent::ResolveInterventionInput { intervention_id: item.id.clone(), action: crate::multi_agent::InterventionAction::Approve, response: None, agent_id: None, snooze_minutes: None }).unwrap();
        assert!(!receipt.resume_project, "group permission cannot start engineering scheduler, including repeated approvals");
    }
    let snapshot = fixture.store.snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.project.status, ProjectStatus::Paused);
    assert_eq!(snapshot.runs.iter().find(|r|r.id==run.id).unwrap().status, RunStatus::Running);
    assert_eq!(snapshot.approvals.iter().find(|a|a.id==approval.id).unwrap().status,"approved");
    assert_eq!(snapshot.tasks.iter().find(|t|t.id==other.id).unwrap().status, other.status);
    assert!(!snapshot.runs.iter().any(|r|r.task_id==other.id));
}

#[test]
fn malformed_turn_metadata_is_recovered_without_human_intervention_or_replaying_members() {
    for footer in [r#"{action:done}"#,r#"{\"action\":\"done\"}"#,r#"{"action":"done","reason":"nothing else"}"#] {
        let fixture=Fixture::new();
        let mut input=fixture.input();input.intent="exchange".into();
        let executor=ScriptedReplies::new(vec![format!("这是已经收到的真实回复。{TURN_CONTROL_OPEN}{footer}{TURN_CONTROL_CLOSE}"),"第二位成员的真实回复。".into()]);
        let snapshot=fixture.run_input(input,&executor);
        assert_eq!(snapshot.rounds[0].status,"completed","format errors are not a user decision");
        let calls=executor.calls.lock().unwrap();assert_eq!(calls.len(),2);
        assert_eq!(calls[0].0,fixture.agents[0]);assert_eq!(calls[1].0,fixture.agents[1]);
        assert!(snapshot.messages.iter().any(|m|m.body=="这是已经收到的真实回复。"));
        assert!(snapshot.messages.iter().all(|m|!m.body.contains(TURN_CONTROL_OPEN)));
    }
}

#[test]
fn malformed_control_without_any_public_reply_is_a_visible_failure() {
    let fixture=Fixture::new();
    let executor=ScriptedReplies::new(vec![format!("{TURN_CONTROL_OPEN}{{action:done}}{TURN_CONTROL_CLOSE}")]);
    let snapshot=fixture.run(&executor);
    assert_eq!(snapshot.rounds[0].status,"failed");
    assert!(snapshot.rounds[0].stop_reason.as_ref().unwrap().contains("未返回可用正文"));
    assert_eq!(executor.calls.lock().unwrap().len(),1);
    assert!(snapshot.messages.iter().all(|m|!m.body.contains(TURN_CONTROL_OPEN)));
}

#[test]
fn hi_calls_selected_members_once_and_keeps_original_scope_constraints() {
    let fixture = Fixture::new();
    let constraint = "仅执行指定模型验收；不调用工具，不修改文件，不访问其他项目";
    {
        let connection = fixture.store.connection.lock().unwrap();
        connection.execute("UPDATE ma_projects SET goal='模型验收专用背景' WHERE id=?1", [&fixture.project]).unwrap();
        connection.execute("UPDATE ma_agents SET description=?2 WHERE id=?1", params![fixture.agents[0], constraint]).unwrap();
    }
    let mut input = fixture.input();
    input.body = "hi".into();
    let executor = ScriptedReplies::new(vec!["hi".into(), "你好".into()]);
    let snapshot = fixture.run_input(input, &executor);
    assert_eq!(snapshot.rounds[0].status, "completed");
    assert_eq!(snapshot.messages.iter().filter(|message| message.sender_type == "agent").count(), 2);
    let calls = executor.calls.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].1.contains(constraint));
    assert!(calls[0].1.contains("尚未迁移的明确旧限制仍须遵守"));
    assert_eq!(snapshot.rounds[0].member_policies[&fixture.agents[0]].boundary.tools, "none");
    assert_eq!(snapshot.rounds[0].member_policies[&fixture.agents[1]].boundary.tools, "research");
    assert!(!calls[0].1.contains("先给出你自己的观点、依据、风险和待确认事项"));
    assert_eq!(fixture.store.agent_profile(&fixture.agents[0]).unwrap().description, constraint);
    assert_eq!(fixture.store.snapshot(&fixture.project).unwrap().project.goal, "模型验收专用背景");
}

#[test]
fn single_addressed_chat_member_never_starts_an_extra_reply() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.body = "@member-1 hi".into();
    input.participant_agent_ids = vec![fixture.agents[1].clone()];
    let executor = ScriptedReplies::new(vec!["你好。@member-0 这只是正文，不是新派发。".into()]);
    let snapshot = fixture.run_input(input, &executor);
    assert_eq!(snapshot.rounds[0].status, "completed");
    assert_eq!(executor.calls.lock().unwrap().len(), 1);
    assert_eq!(snapshot.messages[1].agent_id.as_ref(), Some(&fixture.agents[1]));
}

#[test]
fn exchange_handoffs_follow_real_public_replies_and_stop_without_summary() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.intent = "exchange".into();
    input.body = "@member-0 @member-1 请交流接口方案".into();
    let executor = ScriptedReplies::new(vec![
        controlled_reply("我建议先校验输入。@member-1 你看到什么遗漏？", json!({"action":"continue","nextAgentId":fixture.agents[1],"hasNewInformation":true})),
        controlled_reply("补充：还要验证返回状态。@member-0 这个边界可以吗？", json!({"action":"continue","nextAgentId":fixture.agents[0],"hasNewInformation":true})),
        controlled_reply("可以，返回状态也纳入验证。", json!({"action":"done"})),
    ]);
    let snapshot = fixture.run_input(input, &executor);
    assert_eq!(snapshot.rounds[0].status, "completed");
    let calls = executor.calls.lock().unwrap();
    assert_eq!(calls.iter().map(|call| &call.0).collect::<Vec<_>>(), vec![&fixture.agents[0], &fixture.agents[1], &fixture.agents[0]]);
    assert!(calls[1].1.contains("我建议先校验输入"));
    assert!(calls[2].1.contains("补充：还要验证返回状态"));
    assert!(snapshot.messages.iter().all(|message| !message.body.contains(TURN_CONTROL_OPEN) && !message.body.contains("nextAgentId")));
    let messages = &snapshot.messages;
    assert_eq!(messages[1].reply_to_message_id.as_ref(), Some(&messages[0].id));
    assert_eq!(messages[2].reply_to_message_id.as_ref(), Some(&messages[1].id));
    assert_eq!(messages[3].reply_to_message_id.as_ref(), Some(&messages[2].id));
    assert!(messages.iter().all(|message| message.phase.as_deref() != Some("summary")));
}

#[test]
fn plain_mentions_can_handoff_but_quoted_mentions_and_missing_information_do_not_loop() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.intent = "exchange".into();
    let executor = ScriptedReplies::new(vec![
        "@member-1 请补充一个边界。".into(),
        "无新信息。@member-0 我也没有补充。".into(),
    ]);
    let snapshot = fixture.run_input(input, &executor);
    assert_eq!(executor.calls.lock().unwrap().len(), 2);
    assert_eq!(snapshot.rounds[0].status, "completed");
    assert_eq!(snapshot.messages[2].reply_to_message_id.as_ref(), Some(&snapshot.messages[1].id));
    let round = &snapshot.rounds[0];
    for body in ["> @member-1 引用\n我的回答", "```text\n@member-1\n```", "`@member-1` 是示例"] {
        assert!(mentioned_reply_target(&fixture.store, round, &fixture.agents[0], body).unwrap().is_none());
    }
}

#[test]
fn waiting_for_human_stops_all_automatic_replies_and_silence_has_no_fake_message() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.intent = "exchange".into();
    let executor = ScriptedReplies::new(vec![controlled_reply("目标平台是哪一个？", json!({"action":"wait"}))]);
    let snapshot = fixture.run_input(input, &executor);
    assert_eq!(snapshot.rounds[0].status, "needs_decision");
    assert_eq!(executor.calls.lock().unwrap().len(), 1);
    assert_eq!(snapshot.messages[1].body, "目标平台是哪一个？");

    let fixture = Fixture::new();
    let executor = ScriptedReplies::new(vec![controlled_reply("", json!({"action":"silent"})), "我来回答这个具体问题。".into()]);
    let snapshot = fixture.run(&executor);
    assert_eq!(snapshot.rounds[0].status, "completed");
    assert_eq!(executor.calls.lock().unwrap().len(), 2);
    assert_eq!(snapshot.messages.len(), 2, "a silent turn must not create an empty assistant bubble");
    assert_eq!(snapshot.messages[1].agent_id.as_ref(), Some(&fixture.agents[1]));
    assert_eq!(fixture.store.snapshot(&fixture.project).unwrap().runs.len(), 2, "silent execution evidence remains durable");
}

#[test]
fn ordinary_chat_wait_preserves_every_addressed_members_initial_reply() {
    // Include the exact class of response observed during real-model acceptance,
    // plus a genuine missing-information question. Neither may cancel recipient B.
    for first in ["你好，我在的，有什么想聊的随时说。", "你希望我分析哪个函数？请提供函数名称。"] {
        let fixture = Fixture::new();
        let executor = ScriptedReplies::new(vec![
            controlled_reply(first, json!({"action":"wait"})),
            controlled_reply("我是另一位被选成员，这是我的回答。", json!({"action":"done"})),
        ]);
        let snapshot = fixture.run(&executor);
        let calls = executor.calls.lock().unwrap();
        assert_eq!(calls.iter().map(|call| &call.0).collect::<Vec<_>>(), vec![&fixture.agents[0], &fixture.agents[1]]);
        assert_eq!(snapshot.messages.iter().filter(|message| message.sender_type == "agent").count(), 2);
        assert!(calls[1].1.contains(first), "the second member receives the first actual response");
        assert_eq!(snapshot.rounds[0].status, "needs_decision", "do not erase an explicit human-information wait merely because another member is done");
        assert!(snapshot.rounds[0].stop_reason.as_deref().unwrap().contains("已选成员均已回应"));
    }
}

#[test]
fn courteous_question_marked_done_finishes_ordinary_chat_without_forcing_wait() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.body = "hi".into();
    let executor = ScriptedReplies::new(vec![
        controlled_reply("你好，有什么想聊的吗？", json!({"action":"done"})),
        "你好！".into(),
    ]);
    let snapshot = fixture.run_input(input, &executor);
    assert_eq!(executor.calls.lock().unwrap().len(), 2);
    assert_eq!(snapshot.rounds[0].status, "completed");
    assert!(snapshot.rounds[0].stop_reason.is_none());
}

#[test]
fn invalid_handoff_never_dispatches_an_unselected_or_same_member() {
    for target in ["unselected-member", "SELF"] {
        let fixture = Fixture::new();
        let mut input = fixture.input();
        input.intent = "exchange".into();
        let target = if target == "SELF" { fixture.agents[0].as_str() } else { target };
        let executor = ScriptedReplies::new(vec![controlled_reply("这段实际回复保留。", json!({"action":"continue","nextAgentId":target,"hasNewInformation":true})),"被选中的另一位成员回应。".into()]);
        let snapshot = fixture.run_input(input, &executor);
        assert_eq!(snapshot.rounds[0].status, "completed");
        let calls=executor.calls.lock().unwrap();
        assert_eq!(calls.len(),2);
        assert_eq!(calls[0].0,fixture.agents[0]);assert_eq!(calls[1].0,fixture.agents[1]);
        assert_eq!(snapshot.messages[1].body, "这段实际回复保留。");
    }
}

#[test]
fn no_progress_and_turn_budget_stop_a_valid_exchange_without_extra_model_calls() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.intent = "exchange".into();
    let executor = ScriptedReplies::new(vec![
        controlled_reply("请核对输入约束。", json!({"action":"continue","nextAgentId":fixture.agents[1],"hasNewInformation":true})),
        controlled_reply("我认为输出约束也需要核对。", json!({"action":"continue","nextAgentId":fixture.agents[0],"hasNewInformation":true})),
        controlled_reply("请核对输入约束。", json!({"action":"continue","nextAgentId":fixture.agents[1],"hasNewInformation":true})),
    ]);
    let snapshot = fixture.run_input(input, &executor);
    assert_eq!(snapshot.rounds[0].status, "completed");
    assert_eq!(executor.calls.lock().unwrap().len(), 3);

    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.intent = "exchange".into();
    let replies = (0..4).map(|index| controlled_reply(&format!("新的具体问题 {index}"),
        json!({"action":"continue","nextAgentId":fixture.agents[(index + 1) % 2],"hasNewInformation":true}))).collect();
    let executor = ScriptedReplies::new(replies);
    let snapshot = fixture.run_input(input, &executor);
    assert_eq!(snapshot.rounds[0].status, "needs_decision");
    assert!(snapshot.rounds[0].stop_reason.as_deref().unwrap().contains("上限"));
    assert_eq!(executor.calls.lock().unwrap().len(), 4);
}

#[test]
fn control_fragments_are_hidden_during_streaming_and_final_projection() {
    let fixture = Fixture::new();
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    let turn = prepare_turn(&fixture.store, &round.id, 0, "chat", 0, &fixture.agents[0]).unwrap();
    let run = fixture.store.claim_task(ClaimTaskInput { task_id: turn.task_id, agent_id: fixture.agents[0].clone(), lease_seconds: 60 }).unwrap();
    fixture.store.update_run(UpdateRunInput { run_id: run.id.clone(), status: RunStatus::Running,
        output_summary: None, error: None, session_locator: None }).unwrap();
    for fragment in ["你好\n<crew", "vela-turn>{\"action\":\"done\"}", "</crewvela-turn>"] {
        fixture.store.append_runtime_event(super::super::super::AppendRuntimeEventInput {
            run_id: run.id.clone(), event_type: "assistant_delta".into(), payload: json!({"text":fragment}),
        }).unwrap();
        let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
        assert_eq!(snapshot.messages.iter().find(|message| message.id == turn.id).unwrap().body, "你好");
    }
    fixture.store.finalize_successful_run(&run.id, "isolated", &controlled_reply("你好", json!({"action":"done"})), false, None, None).unwrap();
    assert_eq!(fixture.store.conversation_snapshot(&fixture.project).unwrap().messages[1].body, "你好");
}

#[test]
fn execution_plan_uses_one_actual_planner_and_never_starts_engineering_implicitly() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.mode = "execution_plan".into();
    let draft = fixture.draft();
    let executor = ScriptedReplies::new(vec![json!({"summary":draft.summary,"tasks":draft.tasks}).to_string()]);
    let snapshot = fixture.run_input(input, &executor);
    assert_eq!(snapshot.rounds[0].status, "completed");
    assert_eq!(executor.calls.lock().unwrap().len(), 1);
    assert_eq!(snapshot.execution_plans.len(), 1);
    assert_eq!(snapshot.execution_plans[0].status, "pending");
    assert!(snapshot.execution_plans[0].task_ids.is_empty());
    assert!(fixture.store.snapshot(&fixture.project).unwrap().tasks.iter().all(|task| is_conversation_task(&task.id)));
}

struct DirectedInterjection { input: ConversationSendInput }
impl DiscussionExecutor for DirectedInterjection {
    fn execute(&self, store: &MultiAgentStore, _: &str, run: &str, _: &str, _: &AtomicBool) -> Result<()> {
        store.update_run(UpdateRunInput { run_id: run.into(), status: RunStatus::Running,
            output_summary: None, error: None, session_locator: None })?;
        assert!(store.interrupt_conversation(interjection(&self.input))?);
        assert!(!store.interrupt_conversation(interjection(&self.input))?, "retries must not interrupt twice");
        assert!(store.start_queued_conversation(&self.input.project_id)?.is_none(), "the old process is not yet stopped");
        store.update_run(UpdateRunInput { run_id: run.into(), status: RunStatus::Interrupted,
            output_summary: None, error: Some("confirmed stop".into()), session_locator: None })?;
        Ok(())
    }
}

#[test]
fn directed_interjection_preserves_target_identity_and_starts_once_after_confirmed_stop() {
    let fixture = Fixture::new();
    let mut next = fixture.input();
    next.client_message_id = "directed-interjection".into();
    next.body = "@member-1 请回答这个问题".into();
    next.participant_agent_ids = vec![fixture.agents[1].clone()];
    let stopped = fixture.run(&DirectedInterjection { input: next.clone() });
    assert_eq!(stopped.rounds[0].status, "paused");
    let message = stopped.messages.iter().find(|message| message.client_message_id.as_deref() == Some(next.client_message_id.as_str())).unwrap();
    assert_eq!(message.queued_input.as_ref(), Some(&next));
    assert_eq!(message.status, "queued");
    let message_id = message.id.clone();
    let round = fixture.store.start_queued_conversation(&fixture.project).unwrap().unwrap();
    assert_eq!(round.participant_agent_ids, next.participant_agent_ids);
    assert_eq!(round.user_message_id, message_id);
    let executor = ScriptedReplies::new(vec!["这是我针对新问题的回答。".into()]);
    run_discussion(&fixture.store, &round.id, &Arc::new(AtomicBool::new(false)), &executor).unwrap();
    assert_eq!(executor.calls.lock().unwrap()[0].0, fixture.agents[1]);
    assert!(!fixture.store.begin_conversation(next.clone()).unwrap().1);
    assert!(!fixture.store.interrupt_conversation(interjection(&next)).unwrap());
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    let accepted = snapshot.messages.iter().filter(|message| message.client_message_id.as_deref() == Some(next.client_message_id.as_str())).collect::<Vec<_>>();
    assert_eq!(accepted.len(), 1);
    assert_eq!(accepted[0].id, message_id);
    assert!(accepted[0].queued_input.is_none());
}

#[test]
fn restart_preserves_interjection_but_requires_explicit_same_identity_retry() {
    let fixture = Fixture::new();
    fixture.store.begin_conversation(fixture.input()).unwrap();
    let mut next = fixture.input();
    next.client_message_id = "durable-interjection".into();
    next.body = "@member-1 新的问题".into();
    next.participant_agent_ids = vec![fixture.agents[1].clone()];
    fixture.store.interrupt_conversation(interjection(&next)).unwrap();
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    reopened.recover_conversations_at_startup().unwrap();
    let snapshot = reopened.conversation_snapshot(&fixture.project).unwrap();
    let pending = snapshot.messages.iter().find(|message| message.queued_input.is_some()).unwrap();
    assert_eq!(pending.status, "needs_decision");
    assert_eq!(pending.queued_input.as_ref(), Some(&next));
    assert!(reopened.start_queued_conversation(&fixture.project).unwrap().is_none());
    assert!(reopened.snapshot(&fixture.project).unwrap().runs.is_empty());
    let (round, created) = reopened.begin_conversation(next.clone()).unwrap();
    assert!(created);
    let executor = ScriptedReplies::new(vec!["明确重试后的回答".into()]);
    run_discussion(&reopened, &round.id, &Arc::new(AtomicBool::new(false)), &executor).unwrap();
    assert_eq!(executor.calls.lock().unwrap().len(), 1);
    assert!(!reopened.begin_conversation(next).unwrap().1);
}

#[test]
fn unconfirmed_stop_never_drains_interjection_even_after_confirmation_without_retry() {
    let fixture = Fixture::new();
    let snapshot = fixture.run(&StopDuringRun { unconfirmed: true, interrupt: false });
    let mut next = fixture.input();
    next.client_message_id = "after-unknown-stop".into();
    next.participant_agent_ids = vec![fixture.agents[1].clone()];
    fixture.store.interrupt_conversation(interjection(&next)).unwrap();
    assert!(fixture.store.start_queued_conversation(&fixture.project).unwrap().is_none());
    assert!(fixture.store.begin_conversation(next.clone()).is_err());
    let run = fixture.store.snapshot(&fixture.project).unwrap().runs[0].id.clone();
    fixture.store.confirm_run_stopped(&run, true).unwrap();
    fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert!(fixture.store.start_queued_conversation(&fixture.project).unwrap().is_none());
    assert_eq!(fixture.store.snapshot(&fixture.project).unwrap().runs.len(), 1);
    assert_ne!(snapshot.rounds[0].status, "completed");
    assert!(fixture.store.begin_conversation(next).unwrap().1);
}

#[test]
fn legacy_requests_and_rounds_default_to_chat_without_breaking_receipt_identity() {
    let fixture = Fixture::new();
    let input = fixture.input();
    let mut legacy = serde_json::to_value(&input).unwrap();
    legacy.as_object_mut().unwrap().remove("intent");
    assert!(same_request(&legacy.to_string(), &input));
    let (round, _) = fixture.store.begin_conversation(input).unwrap();
    let mut legacy = serde_json::to_value(round).unwrap();
    legacy.as_object_mut().unwrap().remove("intent");
    legacy["phase"] = json!("opinions");
    let round: ConversationRound = serde_json::from_value(legacy).unwrap();
    assert_eq!(round.intent, "chat");
    assert_eq!(round.phase, "opinions");
}

#[test]
fn concurrent_interjection_retries_and_promotion_keep_one_message_and_one_round() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.client_message_id = "racing-interjection".into();
    input.participant_agent_ids = vec![fixture.agents[1].clone()];
    fixture.store.interrupt_conversation(interjection(&input)).unwrap();
    let barrier = Arc::new(std::sync::Barrier::new(3));
    let store = fixture.store.clone();
    let retry_input = input.clone();
    let retry_barrier = barrier.clone();
    let retry = std::thread::spawn(move || {
        retry_barrier.wait();
        for _ in 0..20 { assert!(!store.interrupt_conversation(interjection(&retry_input)).unwrap()); }
    });
    let store = fixture.store.clone();
    let project = fixture.project.clone();
    let start_barrier = barrier.clone();
    let promotion = std::thread::spawn(move || {
        start_barrier.wait();
        store.start_queued_conversation(&project).unwrap().unwrap()
    });
    barrier.wait();
    retry.join().unwrap();
    let round = promotion.join().unwrap();
    assert_eq!(round.status, "queued", "receipt retries must not stop the newly accepted round");
    assert!(!fixture.store.begin_conversation(input).unwrap().1);
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.rounds.len(), 1);
    assert_eq!(snapshot.messages.len(), 1);
    assert!(fixture.store.snapshot(&fixture.project).unwrap().runs.is_empty());
}

#[test]
fn manual_takeover_before_interjection_dispatch_preserves_request_without_running_it() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.client_message_id = "manual-before-dispatch".into();
    input.participant_agent_ids = vec![fixture.agents[1].clone()];
    fixture.store.interrupt_conversation(interjection(&input)).unwrap();
    fixture.store.set_agent_control(&fixture.project, &fixture.agents[1], super::super::super::AgentControlMode::Manual, None).unwrap();
    assert!(fixture.store.start_queued_conversation(&fixture.project).is_err());
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.messages[0].status, "needs_decision");
    assert_eq!(snapshot.messages[0].queued_input.as_ref(), Some(&input));
    assert!(snapshot.rounds.is_empty());
    assert!(fixture.store.snapshot(&fixture.project).unwrap().runs.is_empty());
}

#[test]
fn clear_history_cannot_revive_a_saved_interjection() {
    let fixture = Fixture::new();
    let input = fixture.input();
    fixture.store.interrupt_conversation(interjection(&input)).unwrap();
    fixture.store.settle_queued_conversation_inputs(&fixture.project, "needs_decision").unwrap();
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    fixture.store.clear_conversation(clear_input(&snapshot)).unwrap();
    assert!(fixture.store.start_queued_conversation(&fixture.project).unwrap().is_none());
    assert!(fixture.store.begin_conversation(input).is_err());
}
