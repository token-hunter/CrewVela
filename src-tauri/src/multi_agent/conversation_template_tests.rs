use super::*;

fn setup() -> (Fixture, ConversationSendInput) {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.participant_agent_ids.truncate(1);
    fixture.store.connection.lock().unwrap().execute(
        "UPDATE ma_agents SET runtime_kind='claude_code' WHERE id=?1", [&input.participant_agent_ids[0]],
    ).unwrap();
    input.task_rules = Some(ConversationTaskRules::default());
    let topic = fixture.store.conversation_snapshot(&fixture.project).unwrap().conversation.id;
    input.expected_conversation_id = Some(topic);
    (fixture, input)
}

#[test]
fn conversation_template_runs_in_saved_topic_with_only_its_context_and_projects_the_actual_reply() {
    let (fixture, mut input) = setup();
    input.body = "Bound topic material A".into();
    let initial = fixture.run_input(input.clone(), &ScriptedReplies::new(vec!["Reviewed A".into()]));
    let original = initial.conversation.id.clone();
    let selected = fixture.store.new_conversation(clear_input(&initial)).unwrap().conversation.id;
    let mut unrelated = input.clone();
    unrelated.client_message_id = "selected-topic".into();
    unrelated.expected_conversation_id = Some(selected.clone());
    unrelated.body = "Private selected topic B".into();
    fixture.run_input(unrelated.clone(), &ScriptedReplies::new(vec!["Reviewed B".into()]));

    input.client_message_id = "template-bound".into();
    input.body = "Inspect the supplied pull request".into();
    assert!(fixture.store.begin_conversation(input.clone()).is_err());
    assert!(fixture.store.begin_conversation_in_topic(input.clone(), TopicDispatch::TemplateTrial).is_err());
    let (round, created) = fixture.store.begin_conversation_in_topic(input.clone(), TopicDispatch::ScheduledTemplate).unwrap();
    assert!(created);
    assert!(!fixture.store.begin_conversation_in_topic(input.clone(), TopicDispatch::ScheduledTemplate).unwrap().1);
    assert_eq!(round.conversation_id, original);
    assert!(round.member_policies.values().all(|p| !p.uses_native_capabilities() && p.allows_tools()));
    unrelated.client_message_id = "cannot-stop-other-topic".into();
    assert!(fixture.store.interrupt_conversation(interjection(&unrelated)).is_err());
    assert!(fixture.store.request_conversation_stop(&fixture.project, "cancelled", "wrong topic", Some(&selected)).is_err());

    let runner = ScriptedReplies::new(vec!["Exact template result".into()]);
    run_discussion(&fixture.store, &round.id, &Arc::new(AtomicBool::new(false)), &runner).unwrap();
    let prompt = &runner.calls.lock().unwrap()[0].1;
    assert!(prompt.contains("Bound topic material A"));
    assert!(!prompt.contains("Private selected topic B"));
    let receipt = fixture.store.template_round(&fixture.project, &original, &input.client_message_id).unwrap().unwrap();
    assert_eq!(receipt.status, "completed", "{:?}", receipt.stop_reason);
    assert!(fixture.store.template_round(&fixture.project, &selected, &input.client_message_id).unwrap().is_none());
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.conversation.id, selected);
    assert!(!snapshot.messages.iter().any(|m| m.round_id.as_deref() == Some(&round.id)));
    let source = public_messages_for_topic(&fixture.store.connection.lock().unwrap(), &fixture.project, &original).unwrap();
    assert!(source.iter().any(|m| m.body == "Exact template result" && m.status == "completed" && m.round_id.as_deref() == Some(&round.id)));
}

#[test]
fn conversation_template_rejects_deleted_topics_unsupported_members_and_native_tools() {
    let (fixture, input) = setup();
    let topic = input.expected_conversation_id.as_deref().unwrap();
    assert!(fixture.store.validate_template_destination(&fixture.project, topic, &fixture.agents[0]).is_ok());
    assert!(fixture.store.validate_template_destination(&fixture.project, topic, &fixture.agents[1]).is_err());
    let mut unrestricted = input.clone(); unrestricted.native_capabilities = true;
    assert!(fixture.store.begin_conversation_in_topic(unrestricted, TopicDispatch::ScheduledTemplate).is_err());
    fixture.store.set_project_status(&fixture.project, ProjectStatus::Paused).unwrap();
    for dispatch in [TopicDispatch::TemplateTrial, TopicDispatch::ScheduledTemplate] {
        assert!(fixture.store.begin_conversation_in_topic(input.clone(), dispatch).unwrap_err().to_string().contains("项目已暂停"));
    }
    fixture.store.set_project_status(&fixture.project, ProjectStatus::Active).unwrap();
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    fixture.store.delete_conversation(clear_input(&snapshot)).unwrap();
    assert!(fixture.store.validate_template_destination(&fixture.project, topic, &fixture.agents[0]).is_err());
    assert!(fixture.store.begin_conversation_in_topic(input, TopicDispatch::ScheduledTemplate).is_err());
    assert!(fixture.store.conversation_snapshot(&fixture.project).unwrap().rounds.is_empty());
}

#[test]
fn conversation_template_claim_rechecks_pause_and_changed_runtime_before_starting_a_run() {
    let (fixture, input) = setup();
    let (round, _) = fixture.store.begin_conversation_in_topic(input, TopicDispatch::TemplateTrial).unwrap();
    let member = &fixture.agents[0];
    let turn = prepare_turn(&fixture.store, &round.id, 0, "chat", 1, member).unwrap();
    let claim = || fixture.store.claim_task(ClaimTaskInput { task_id: turn.task_id.clone(), agent_id: member.clone(), lease_seconds: 120 });
    fixture.store.set_project_status(&fixture.project, ProjectStatus::Paused).unwrap();
    assert!(claim().unwrap_err().to_string().contains("项目已暂停"));
    fixture.store.set_project_status(&fixture.project, ProjectStatus::Active).unwrap();
    fixture.store.connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind='opencode' WHERE id=?1", [member]).unwrap();
    assert!(claim().unwrap_err().to_string().contains("受限研究"));
    assert!(fixture.store.snapshot(&fixture.project).unwrap().runs.is_empty());
}
