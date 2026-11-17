use super::*;

fn pending_plan(fixture: &Fixture) -> ConversationExecutionPlan {
    let snapshot = fixture.run(&Replies::default());
    let draft = fixture.draft();
    fixture
        .store
        .save_conversation_plan(
            &snapshot.rounds[0].id,
            &snapshot.messages.last().unwrap().id,
            &json!({"summary": draft.summary, "tasks": draft.tasks}).to_string(),
        )
        .unwrap()
}

#[test]
fn ordinary_chat_keeps_pending_plan_approvable_without_creating_tasks() {
    let fixture = Fixture::new();
    fixture.git();
    let plan = pending_plan(&fixture);
    let mut input = fixture.input();
    input.client_message_id = "ordinary-followup".into();
    input.body = "hi, what does the acceptance check mean?".into();
    let chat = fixture.run_input(input, &Replies::default());
    assert_eq!(
        chat.execution_plans
            .iter()
            .find(|item| item.id == plan.id)
            .unwrap()
            .status,
        "pending"
    );
    assert!(fixture
        .store
        .snapshot(&fixture.project)
        .unwrap()
        .tasks
        .iter()
        .all(|task| is_conversation_task(&task.id)));
    let tasks = fixture
        .store
        .apply_conversation_plan(&ConversationExecutePlanInput {
            project_id: fixture.project.clone(),
            plan_id: plan.id,
            client_request_id: "confirm-after-chat".into(),
        })
        .unwrap();
    assert_eq!(tasks.len(), 1);
}

#[test]
fn explicit_new_plan_request_supersedes_unconfirmed_plan() {
    let fixture = Fixture::new();
    let plan = pending_plan(&fixture);
    let mut input = fixture.input();
    input.client_message_id = "new-proposal".into();
    input.mode = "execution_plan".into();
    fixture.store.begin_conversation(input).unwrap();
    let chat = fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap();
    assert_eq!(
        chat.execution_plans
            .iter()
            .find(|item| item.id == plan.id)
            .unwrap()
            .status,
        "superseded"
    );
    assert!(fixture
        .store
        .snapshot(&fixture.project)
        .unwrap()
        .tasks
        .iter()
        .all(|task| is_conversation_task(&task.id)));
}

#[test]
fn durable_interruption_preserves_chat_plan_but_invalidates_for_explicit_replanning() {
    for mode in ["discussion", "execution_plan"] {
        let fixture = Fixture::new();
        let plan = pending_plan(&fixture);
        let mut active = fixture.input();
        active.client_message_id = "active-chat".into();
        fixture.store.begin_conversation(active).unwrap();
        fixture
            .store
            .interrupt_conversation(ConversationInterruptInput {
                project_id: fixture.project.clone(),
                client_message_id: "interruption".into(),
                body: "new request".into(),
                participant_agent_ids: Some(fixture.agents.clone()),
                mode: Some(mode.into()),
                intent: Some("chat".into()),
                max_rounds: Some(1),
                max_duration_seconds: Some(60),
            })
            .unwrap();
        let chat = fixture
            .store
            .conversation_snapshot(&fixture.project)
            .unwrap();
        assert_eq!(
            chat.execution_plans
                .iter()
                .find(|item| item.id == plan.id)
                .unwrap()
                .status,
            if mode == "execution_plan" {
                "superseded"
            } else {
                "pending"
            }
        );
    }
}
