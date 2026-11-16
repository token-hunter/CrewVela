use super::*;
use crate::multi_agent::{
    AgentRole, CreateAgentInput, CreateProjectInput, CreateTaskInput, PermissionProfile,
    ProjectStatus, RunStatus, SendCollaborationMessageInput, UpdateRunInput,
};

#[path = "conversation_chat_tests.rs"]
mod natural_chat;
#[path = "conversation_live_tests.rs"]
mod live_chat;

struct Fixture {
    store: MultiAgentStore,
    root: std::path::PathBuf,
    project: String,
    agents: Vec<String>,
}
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("crewvela-conversation-{}", id()));
        Self::at(root)
    }
    fn at(root: std::path::PathBuf) -> Self {
        std::fs::create_dir_all(root.join("project")).unwrap();
        let store = MultiAgentStore::open_at(root.join("store.db")).unwrap();
        let project = store
            .create_project(CreateProjectInput {
                name: "Team".into(),
                goal: "bounded public conversation".into(),
                root_path: root.join("project").display().to_string(),
                default_branch: "main".into(),
            })
            .unwrap()
            .id;
        let agents = (0..2)
            .map(|index| {
                store
                    .create_agent(CreateAgentInput {
                        avatar_id: None,
                        project_id: project.clone(),
                        name: format!("member-{index}"),
                        runtime_kind: AgentRuntimeKind::Codex,
                        role: AgentRole::Worker,
                        description: "explicit team member".into(),
                        persona: Default::default(),
                        persona_custom: String::new(),
                        model: None,
                        permission_profile: PermissionProfile::Yolo,
                        max_concurrency: 1,
                    })
                    .unwrap()
                    .id
            })
            .collect();
        Self {
            store,
            root,
            project,
            agents,
        }
    }
    fn input(&self) -> ConversationSendInput {
        ConversationSendInput {
            project_id: self.project.clone(),
            client_message_id: "user-1".into(),
            body: "Discuss the project".into(),
            participant_agent_ids: self.agents.clone(),
            mode: "discussion".into(),
            intent: "chat".into(),
            max_rounds: 1,
            max_duration_seconds: 60,
        }
    }
    fn run(&self, executor: &impl DiscussionExecutor) -> ConversationSnapshot {
        self.run_input(self.input(), executor)
    }
    fn run_input(&self, input: ConversationSendInput, executor: &impl DiscussionExecutor) -> ConversationSnapshot {
        let (round, _) = self.store.begin_conversation(input).unwrap();
        run_discussion(
            &self.store,
            &round.id,
            &Arc::new(AtomicBool::new(false)),
            executor,
        )
        .unwrap();
        self.store.conversation_snapshot(&self.project).unwrap()
    }
    fn draft(&self) -> PlanDraft {
        PlanDraft {
            summary: "explicit plan".into(),
            tasks: vec![ConversationPlannedTask {
                key: "one".into(),
                title: "implement".into(),
                description: "owned files".into(),
                acceptance_criteria: "verify behavior".into(),
                verification_command: Some("node --test tests/owned.test.mjs".into()),
                assignee_agent_id: self.agents[0].clone(),
                reviewer_agent_id: None,
                workspace_mode: WorkspaceMode::Worktree,
                write_paths: vec!["src/owned".into()],
                depends_on: vec![],
            }],
        }
    }
    fn git(&self) {
        for arguments in [
            vec!["init", "-b", "main"],
            vec![
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@local",
                "commit",
                "--allow-empty",
                "-m",
                "initial",
            ],
        ] {
            let result = std::process::Command::new("git")
                .arg("-C")
                .arg(self.root.join("project"))
                .args(arguments)
                .output()
                .unwrap();
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Drop the live connection before best-effort fixture removal.
        let connection = self.store.connection.lock().unwrap();
        let _ = connection.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
        drop(connection);
        // Windows retains open SQLite files until the enclosing fixture is dropped.
    }
}

#[test]
fn conversation_rejects_manually_controlled_member_before_any_dispatch() {
    let fixture = Fixture::new();
    let target = &fixture.agents[1];
    fixture.store.set_agent_control(&fixture.project, target, super::super::AgentControlMode::Manual, None).unwrap();
    let result = fixture.store.begin_conversation(fixture.input());
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert!(result.is_err(), "a manual member must be rejected before starting the first member");
    let error = result.unwrap_err().to_string();
    assert!(error.contains("member-1") && error.contains("接管"), "{error}");
    assert!(snapshot.rounds.is_empty() && snapshot.messages.is_empty());
    assert!(fixture.store.snapshot(&fixture.project).unwrap().tasks.is_empty());
    fixture.store.set_agent_control(&fixture.project, target, super::super::AgentControlMode::Auto, None).unwrap();
    let (round, started) = fixture.store.begin_conversation(fixture.input()).unwrap();
    assert!(started);
    // An IPC retry must still return its accepted receipt after state changes.
    fixture.store.set_agent_control(&fixture.project, target, super::super::AgentControlMode::Manual, None).unwrap();
    let (receipt, restarted) = fixture.store.begin_conversation(fixture.input()).unwrap();
    assert_eq!(receipt.id, round.id);
    assert!(!restarted);
}
#[derive(Default)]
struct Replies {
    prompts: Mutex<Vec<String>>,
    repeated: bool,
}

struct ScriptedReplies {
    replies: Vec<String>,
    calls: Mutex<Vec<(String, String)>>,
}
impl ScriptedReplies {
    fn new(replies: Vec<String>) -> Self { Self { replies, calls: Mutex::new(Vec::new()) } }
}
impl DiscussionExecutor for ScriptedReplies {
    fn execute(&self, store: &MultiAgentStore, task: &str, run: &str, agent: &str, _: &AtomicBool) -> Result<()> {
        let project = store.agent_profile(agent)?.project_id;
        let prompt = store.snapshot(&project)?.tasks.into_iter().find(|item| item.id == task).unwrap().description;
        let mut calls = self.calls.lock().unwrap();
        let output = self.replies.get(calls.len()).context("unexpected additional model call")?.clone();
        calls.push((agent.into(), prompt));
        drop(calls);
        store.update_run(UpdateRunInput { run_id: run.into(), status: RunStatus::Running,
            output_summary: None, error: None, session_locator: None })?;
        store.finalize_successful_run(run, "isolated", &output, false, None, None)
    }
}

fn controlled_reply(body: &str, control: Value) -> String {
    format!("{body}\n{TURN_CONTROL_OPEN}{control}{TURN_CONTROL_CLOSE}")
}

fn interjection(input: &ConversationSendInput) -> ConversationInterruptInput {
    ConversationInterruptInput { project_id: input.project_id.clone(), client_message_id: input.client_message_id.clone(),
        body: input.body.clone(), participant_agent_ids: Some(input.participant_agent_ids.clone()), mode: Some(input.mode.clone()),
        intent: Some(input.intent.clone()), max_rounds: Some(input.max_rounds), max_duration_seconds: Some(input.max_duration_seconds) }
}
impl DiscussionExecutor for Replies {
    fn execute(
        &self,
        store: &MultiAgentStore,
        task: &str,
        run: &str,
        agent: &str,
        _: &AtomicBool,
    ) -> Result<()> {
        let project = store.agent_profile(agent)?.project_id;
        let snapshot = store.snapshot(&project)?;
        let prompt = snapshot
            .tasks
            .iter()
            .find(|t| t.id == task)
            .unwrap()
            .description
            .clone();
        let mut prompts = self.prompts.lock().unwrap();
        prompts.push(prompt);
        let count = prompts.len();
        drop(prompts);
        store.update_run(UpdateRunInput {
            run_id: run.into(),
            status: RunStatus::Running,
            output_summary: None,
            error: None,
            session_locator: None,
        })?;
        store.finalize_successful_run(
            run,
            "isolated",
            &if self.repeated {
                "same reply".into()
            } else {
                format!("Actual fixture reply {count}")
            },
            false,
            None,
            None,
        )
    }
}

#[test]
fn public_chat_calls_each_addressed_member_once_with_durable_identity() {
    let fixture = Fixture::new();
    let executor = Replies::default();
    let snapshot = fixture.run(&executor);
    assert_eq!(snapshot.rounds[0].status, "completed");
    assert_eq!(snapshot.messages.len(), 3);
    let replies = snapshot
        .messages
        .iter()
        .filter(|m| m.sender_type == "agent")
        .collect::<Vec<_>>();
    assert_eq!(
        replies
            .iter()
            .map(|m| m.phase.as_deref().unwrap())
            .collect::<Vec<_>>(),
        ["chat", "chat"]
    );
    assert!(replies.iter().all(|m| m.run_id.is_some()
        && m.task_id.is_some()
        && m.reply_to_message_id.is_some()
        && m.status == "completed"));
    assert!(executor.prompts.lock().unwrap()[1].contains("Actual fixture reply 1"));
    assert!(replies.iter().all(|reply| reply.reply_to_message_id.as_deref() == Some(snapshot.rounds[0].user_message_id.as_str())));
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    let restored = reopened.conversation_snapshot(&fixture.project).unwrap();
    assert_eq!(restored.messages.len(), 3);
    assert_eq!(restored.conversation.id, snapshot.conversation.id);
    assert_eq!(
        reopened.snapshot(&fixture.project).unwrap().project.status,
        ProjectStatus::Active
    );
}

fn clear_input(snapshot: &ConversationSnapshot) -> ConversationClearInput {
    ConversationClearInput {
        project_id: snapshot.conversation.project_id.clone(),
        expected_conversation_id: snapshot.conversation.id.clone(),
        expected_message_ids: snapshot.messages.iter().map(|message| message.id.clone()).collect(),
    }
}

#[test]
fn new_conversation_preserves_old_evidence_and_isolates_new_context_durably() {
    let fixture = Fixture::new();
    let previous = fixture.run(&Replies::default());
    let project = serde_json::to_value(fixture.store.snapshot(&fixture.project).unwrap()).unwrap();
    let fresh = fixture.store.new_conversation(clear_input(&previous)).unwrap();
    assert_ne!(fresh.conversation.id, previous.conversation.id);
    assert!(fresh.messages.is_empty() && fresh.rounds.is_empty() && fresh.execution_plans.is_empty());
    assert_eq!(project, serde_json::to_value(fixture.store.snapshot(&fixture.project).unwrap()).unwrap());
    let connection = fixture.store.connection.lock().unwrap();
    for message in &previous.messages {
        assert_eq!(message_with(&connection, &message.id).unwrap().conversation_id, previous.conversation.id);
    }
    drop(connection);
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    assert_eq!(reopened.conversation_snapshot(&fixture.project).unwrap().conversation.id, fresh.conversation.id);
    assert!(reopened.conversation_snapshot(&fixture.project).unwrap().messages.is_empty());
    let mut input = fixture.input(); input.client_message_id = "fresh-conversation-topic".into(); input.body = "NEW_SESSION_ONLY".into();
    let executor = Replies::default();
    let result = fixture.run_input(input, &executor);
    assert!(result.messages.iter().all(|message| message.conversation_id == fresh.conversation.id));
    let prompts = executor.prompts.lock().unwrap();
    assert!(prompts[0].contains("NEW_SESSION_ONLY"));
    assert!(!prompts[0].contains("Discuss the project") && !prompts[0].contains("Actual fixture reply"));
}

#[test]
fn new_conversation_allows_empty_sessions_and_rejects_stale_or_active_requests() {
    let fixture = Fixture::new();
    let empty = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    let fresh = fixture.store.new_conversation(clear_input(&empty)).unwrap();
    assert_ne!(fresh.conversation.id, empty.conversation.id);
    assert!(fixture.store.new_conversation(clear_input(&empty)).unwrap_err().to_string().contains("已变化"));
    assert_eq!(fixture.store.conversation_snapshot(&fixture.project).unwrap().conversation.id, fresh.conversation.id);
    let finished = fixture.run(&Replies::default());
    for status in ACTIVE {
        let connection = fixture.store.connection.lock().unwrap();
        let mut round = finished.rounds[0].clone(); round.status = (*status).into();
        save_round(&connection, &round).unwrap(); drop(connection);
        assert!(fixture.store.new_conversation(clear_input(&finished)).unwrap_err().to_string().contains("暂不能新建会话"), "{status}");
    }
    let connection = fixture.store.connection.lock().unwrap();
    save_round(&connection, &finished.rounds[0]).unwrap();
    connection.execute("UPDATE ma_runs SET status='running' WHERE project_id=?1", [&fixture.project]).unwrap();
    drop(connection);
    assert!(fixture.store.new_conversation(clear_input(&finished)).unwrap_err().to_string().contains("暂不能新建会话"));
}

#[test]
fn clear_history_is_durable_project_scoped_and_preserves_execution_evidence() {
    let fixture = Fixture::new();
    fixture.git();
    let initial = fixture.run(&Replies::default());
    let draft = fixture.draft();
    let plan = fixture.store.save_conversation_plan(&initial.rounds[0].id,
        &initial.messages.last().unwrap().id, &json!({"summary":draft.summary,"tasks":draft.tasks}).to_string()).unwrap();
    let approval = ConversationExecutePlanInput {project_id: fixture.project.clone(), plan_id: plan.id, client_request_id: "execute-before-clear".into()};
    let tasks = fixture.store.apply_conversation_plan(&approval).unwrap();
    let run = fixture.store.claim_task(ClaimTaskInput {task_id: tasks[0].clone(), agent_id: fixture.agents[0].clone(), lease_seconds: 120}).unwrap();
    // Engineering execution may continue; clearing never stops or resets it.
    let before = fixture.store.snapshot(&fixture.project).unwrap();
    let other = fixture.store.create_project(CreateProjectInput {name:"Other".into(),goal:"unrelated".into(),root_path:fixture.root.display().to_string(),default_branch:"main".into()}).unwrap();
    fixture.store.interrupt_conversation(ConversationInterruptInput {project_id: other.id.clone(),client_message_id:"other-message".into(),body:"KEEP_OTHER_PROJECT".into(),..Default::default()}).unwrap();
    let other_before = fixture.store.conversation_snapshot(&other.id).unwrap();
    let current = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert!(!current.messages.is_empty());
    let cleared = fixture.store.clear_conversation(clear_input(&current)).unwrap();
    assert_ne!(cleared.conversation.id, current.conversation.id);
    assert!(cleared.messages.is_empty() && cleared.rounds.is_empty() && cleared.execution_plans.is_empty());
    assert_eq!(serde_json::to_value(&before).unwrap(), serde_json::to_value(fixture.store.snapshot(&fixture.project).unwrap()).unwrap());
    assert_eq!(serde_json::to_value(other_before).unwrap(), serde_json::to_value(fixture.store.conversation_snapshot(&other.id).unwrap()).unwrap());
    assert!(fixture.store.apply_conversation_plan(&approval).unwrap_err().to_string().contains("已清空"));
    // A late engineering result must not be re-published into the new history.
    fixture.store.update_run(UpdateRunInput {run_id:run.id.clone(),status:RunStatus::Running,output_summary:None,error:None,session_locator:None}).unwrap();
    fixture.store.finalize_successful_run(&run.id,"isolated","OLD_EXECUTION_OUTPUT",false,None,None).unwrap();
    for _ in 0..3 { assert!(fixture.store.conversation_snapshot(&fixture.project).unwrap().messages.is_empty()); }
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    assert_eq!(reopened.conversation_snapshot(&fixture.project).unwrap().conversation.id, cleared.conversation.id);
    assert!(reopened.conversation_snapshot(&fixture.project).unwrap().messages.is_empty());
    let connection = fixture.store.connection.lock().unwrap();
    let retained: i64 = connection.query_row("SELECT COUNT(*) FROM ma_conversation_messages WHERE project_id=?1",[&fixture.project],|row|row.get(0)).unwrap();
    assert!(retained >= current.messages.len() as i64, "Original evidence is retained, not deleted");
    drop(connection);
    // Retrying a pre-clear send stays idempotent and cannot replay its execution.
    assert!(!fixture.store.begin_conversation(fixture.input()).unwrap().1);
    let mut input = fixture.input(); input.client_message_id = "new-topic".into(); input.body = "NEW_PUBLIC_TOPIC".into();
    let (round, fresh) = fixture.store.begin_conversation(input).unwrap(); assert!(fresh);
    let executor = Replies::default();
    run_discussion(&fixture.store,&round.id,&Arc::new(AtomicBool::new(false)),&executor).unwrap();
    let prompts = executor.prompts.lock().unwrap();
    assert!(prompts[0].contains("NEW_PUBLIC_TOPIC"));
    assert!(!prompts[0].contains("Discuss the project") && !prompts[0].contains("OLD_EXECUTION_OUTPUT") && !prompts[0].contains("Actual fixture reply"));
    let latest = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert_eq!(latest.rounds.len(), 1);
    assert_eq!(latest.messages.len(), 3);
    assert!(latest.messages.iter().all(|message| message.conversation_id == cleared.conversation.id));
}

#[test]
fn clear_history_rejects_active_discussions_and_stale_confirmation() {
    let fixture = Fixture::new();
    let initial = fixture.run(&Replies::default());
    for status in ACTIVE {
        let connection = fixture.store.connection.lock().unwrap();
        let mut round = initial.rounds[0].clone(); round.status = (*status).into(); save_round(&connection,&round).unwrap(); drop(connection);
        assert!(fixture.store.clear_conversation(clear_input(&initial)).unwrap_err().to_string().contains("暂不能清空"), "{status}");
    }
    let connection = fixture.store.connection.lock().unwrap();
    save_round(&connection,&initial.rounds[0]).unwrap();
    let run: String = connection.query_row("SELECT id FROM ma_runs WHERE project_id=?1 LIMIT 1",[&fixture.project],|row|row.get(0)).unwrap();
    connection.execute("UPDATE ma_runs SET status='running' WHERE id=?1",[&run]).unwrap(); drop(connection);
    assert!(fixture.store.clear_conversation(clear_input(&initial)).unwrap_err().to_string().contains("暂不能清空"));
    let connection = fixture.store.connection.lock().unwrap();
    connection.execute("UPDATE ma_runs SET status='succeeded' WHERE id=?1",[&run]).unwrap(); drop(connection);
    fixture.store.interrupt_conversation(ConversationInterruptInput {project_id: fixture.project.clone(),client_message_id:"arrived-after-confirm".into(),body:"NEW_MESSAGE_AFTER_CONFIRM".into(),..Default::default()}).unwrap();
    assert!(fixture.store.clear_conversation(clear_input(&initial)).unwrap_err().to_string().contains("已变化"));
    let latest = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    let cleared = fixture.store.clear_conversation(clear_input(&latest)).unwrap();
    assert!(fixture.store.clear_conversation(clear_input(&latest)).unwrap_err().to_string().contains("已变化"));
    assert!(fixture.store.conversation_snapshot(&fixture.project).unwrap().messages.is_empty());
    assert_eq!(fixture.store.conversation_snapshot(&fixture.project).unwrap().conversation.id, cleared.conversation.id);
    let mut wrong = clear_input(&cleared); wrong.project_id = "missing-project".into();
    assert!(fixture.store.clear_conversation(wrong).is_err());
}

#[test]
fn clearing_hides_pending_plans_and_prevents_stale_approval() {
    let fixture = Fixture::new();
    let initial = fixture.run(&Replies::default());
    let draft = fixture.draft();
    let plan = fixture.store.save_conversation_plan(&initial.rounds[0].id,
        &initial.messages.last().unwrap().id,&json!({"summary":draft.summary,"tasks":draft.tasks}).to_string()).unwrap();
    let current = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    fixture.store.clear_conversation(clear_input(&current)).unwrap();
    assert!(fixture.store.apply_conversation_plan(&ConversationExecutePlanInput {project_id:fixture.project.clone(),plan_id:plan.id,client_request_id:"stale-approval".into()}).unwrap_err().to_string().contains("已清空"));
    assert!(fixture.store.conversation_snapshot(&fixture.project).unwrap().execution_plans.is_empty());
}

#[test]
fn clear_and_new_send_serialize_without_losing_the_new_request() {
    let fixture = Fixture::new();
    let initial = fixture.run(&Replies::default());
    let barrier = Arc::new(std::sync::Barrier::new(2));
    let store = fixture.store.clone();
    let clear = clear_input(&initial);
    let clear_barrier = barrier.clone();
    let clear_thread = std::thread::spawn(move || { clear_barrier.wait(); store.clear_conversation(clear) });
    let store = fixture.store.clone();
    let mut input = fixture.input(); input.client_message_id = "concurrent-new-topic".into(); input.body = "MUST_REMAIN_VISIBLE".into();
    let send_thread = std::thread::spawn(move || { barrier.wait(); store.begin_conversation(input) });
    let clear_result = clear_thread.join().unwrap();
    let (round, created) = send_thread.join().unwrap().unwrap(); assert!(created);
    let current = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert!(current.messages.iter().any(|message| message.body == "MUST_REMAIN_VISIBLE"));
    assert_eq!(current.conversation.id, round.conversation_id);
    assert_eq!(current.rounds.len(), if clear_result.is_ok() {1} else {2});
    if let Err(error) = clear_result { assert!(error.to_string().contains("暂不能清空")); }
}

#[test]
fn retries_do_not_duplicate_human_messages_or_dispatch_again() {
    let fixture = Fixture::new();
    let first = fixture.store.begin_conversation(fixture.input()).unwrap();
    let second = fixture.store.begin_conversation(fixture.input()).unwrap();
    assert!(first.1);
    assert!(!second.1);
    assert_eq!(first.0.id, second.0.id);
    run_discussion(
        &fixture.store,
        &first.0.id,
        &Arc::new(AtomicBool::new(false)),
        &Replies::default(),
    )
    .unwrap();
    assert!(!fixture.store.begin_conversation(fixture.input()).unwrap().1);
    let mut changed = fixture.input();
    changed.body = "different".into();
    assert!(fixture.store.begin_conversation(changed).is_err());
    assert_eq!(
        fixture.store.snapshot(&fixture.project).unwrap().runs.len(),
        2
    );
}

#[test]
fn concurrent_retries_share_one_durable_round() {
    let fixture = Fixture::new();
    let input = fixture.input();
    let handles = (0..4)
        .map(|_| {
            let store = fixture.store.clone();
            let input = input.clone();
            std::thread::spawn(move || store.begin_conversation(input).unwrap())
        })
        .collect::<Vec<_>>();
    let results = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(results.iter().filter(|r| r.1).count(), 1);
    assert!(results.iter().all(|r| r.0.id == results[0].0.id));
}

#[test]
fn private_mail_is_not_imported_into_public_context_or_output() {
    let fixture = Fixture::new();
    fixture
        .store
        .send_collaboration_message(SendCollaborationMessageInput {
            project_id: fixture.project.clone(),
            task_id: None,
            sender_type: "agent".into(),
            sender_id: Some(fixture.agents[0].clone()),
            target_agent_id: Some(fixture.agents[1].clone()),
            body: "PRIVATE_DO_NOT_COPY".into(),
        })
        .unwrap();
    let executor = Replies::default();
    let snapshot = fixture.run(&executor);
    assert!(snapshot
        .messages
        .iter()
        .all(|m| !m.body.contains("PRIVATE_DO_NOT_COPY")));
    assert!(executor
        .prompts
        .lock()
        .unwrap()
        .iter()
        .all(|p| !p.contains("PRIVATE_DO_NOT_COPY")));
}

#[test]
fn bounded_rounds_members_and_unsupported_runtime_reject_before_persistence() {
    let fixture = Fixture::new();
    for (rounds, duration) in [(0, 60), (4, 60), (1, 59), (1, 1801)] {
        let mut input = fixture.input();
        input.max_rounds = rounds;
        input.max_duration_seconds = duration;
        assert!(fixture.store.begin_conversation(input).is_err());
    }
    let mut input = fixture.input();
    input
        .participant_agent_ids
        .push(input.participant_agent_ids[0].clone());
    assert!(fixture.store.begin_conversation(input).is_err());
    fixture
        .store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE ma_agents SET runtime_kind='kimi_code' WHERE id=?1",
            [&fixture.agents[0]],
        )
        .unwrap();
    assert!(fixture
        .store
        .begin_conversation(fixture.input())
        .unwrap_err()
        .to_string()
        .contains("禁止写入"));
    assert!(fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap()
        .messages
        .is_empty());
}

#[test]
fn discussion_tasks_cannot_be_claimed_by_general_scheduler() {
    let fixture = Fixture::new();
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    let turn = prepare_turn(
        &fixture.store,
        &round.id,
        0,
        "opinions",
        0,
        &fixture.agents[0],
    )
    .unwrap();
    assert!(fixture
        .store
        .runnable_tasks(&fixture.project)
        .unwrap()
        .is_empty());
    assert_eq!(
        prepare_turn(
            &fixture.store,
            &round.id,
            0,
            "opinions",
            0,
            &fixture.agents[0]
        )
        .unwrap()
        .task_id,
        turn.task_id
    );
    assert_eq!(
        fixture
            .store
            .snapshot(&fixture.project)
            .unwrap()
            .tasks
            .len(),
        1
    );
}

struct StopDuringRun {
    unconfirmed: bool,
    interrupt: bool,
}
impl DiscussionExecutor for StopDuringRun {
    fn execute(
        &self,
        store: &MultiAgentStore,
        _: &str,
        run: &str,
        agent: &str,
        cancelled: &AtomicBool,
    ) -> Result<()> {
        let project = store.agent_profile(agent)?.project_id;
        store.update_run(UpdateRunInput {
            run_id: run.into(),
            status: RunStatus::Running,
            output_summary: None,
            error: None,
            session_locator: None,
        })?;
        if self.unconfirmed {
            store.mark_run_stop_unverified(run, "fake process did not stop")?;
            bail!("unconfirmed");
        }
        if self.interrupt {
            store.interrupt_conversation(ConversationInterruptInput {
                project_id: project,
                client_message_id: "interjection".into(),
                body: "human correction".into(),
                ..Default::default()
            })?;
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while !cancelled.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(cancelled.load(Ordering::Acquire));
        store.update_run(UpdateRunInput {
            run_id: run.into(),
            status: RunStatus::Cancelled,
            output_summary: None,
            error: Some("confirmed stop".into()),
            session_locator: None,
        })?;
        bail!("cancelled")
    }
}

#[test]
fn human_interjection_is_durable_stops_followups_and_allows_explicit_new_round() {
    let fixture = Fixture::new();
    let snapshot = fixture.run(&StopDuringRun {
        unconfirmed: false,
        interrupt: true,
    });
    assert_eq!(snapshot.rounds[0].status, "paused");
    assert!(snapshot
        .messages
        .iter()
        .any(|m| m.sender_type == "user" && m.body == "human correction"));
    assert_eq!(
        fixture.store.snapshot(&fixture.project).unwrap().runs.len(),
        1
    );
    let mut next = fixture.input();
    next.client_message_id = "user-2".into();
    let (round, _) = fixture.store.begin_conversation(next).unwrap();
    let executor = Replies::default();
    run_discussion(
        &fixture.store,
        &round.id,
        &Arc::new(AtomicBool::new(false)),
        &executor,
    )
    .unwrap();
    assert!(executor.prompts.lock().unwrap()[0].contains("human correction"));
}

#[test]
fn time_limit_cancels_current_executor_before_finishing_round() {
    let fixture = Fixture::new();
    let (mut round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    round.deadline_at = now() + 100;
    save_round(&fixture.store.connection.lock().unwrap(), &round).unwrap();
    run_discussion(
        &fixture.store,
        &round.id,
        &Arc::new(AtomicBool::new(false)),
        &StopDuringRun {
            unconfirmed: false,
            interrupt: false,
        },
    )
    .unwrap();
    let snapshot = fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap();
    assert_eq!(snapshot.rounds[0].status, "needs_decision");
    assert!(snapshot.rounds[0]
        .stop_reason
        .as_deref()
        .unwrap()
        .contains("时间上限"));
    assert_eq!(
        fixture.store.snapshot(&fixture.project).unwrap().runs.len(),
        1
    );
}

#[test]
fn unconfirmed_stop_is_visible_and_blocks_new_round() {
    let fixture = Fixture::new();
    let snapshot = fixture.run(&StopDuringRun {
        unconfirmed: true,
        interrupt: false,
    });
    assert_eq!(snapshot.rounds[0].status, "unconfirmed");
    let mut next = fixture.input();
    next.client_message_id = "user-2".into();
    assert!(fixture.store.begin_conversation(next).is_err());
    assert!(snapshot.messages.iter().any(|m| m.status == "unconfirmed"));
}

#[test]
fn identical_greetings_do_not_start_mutual_commentary_or_summary() {
    let fixture = Fixture::new();
    let snapshot = fixture.run(&Replies {
        repeated: true,
        ..Default::default()
    });
    assert_eq!(snapshot.rounds[0].status, "completed");
    assert_eq!(
        fixture.store.snapshot(&fixture.project).unwrap().runs.len(),
        2
    );
    assert!(!snapshot
        .messages
        .iter()
        .any(|m| m.phase.as_deref() == Some("summary")));
}

#[test]
fn native_reply_ingestion_matches_exact_run_and_ignores_reasoning_and_replay() {
    let fixture = Fixture::new();
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    let turn = prepare_turn(
        &fixture.store,
        &round.id,
        0,
        "opinions",
        0,
        &fixture.agents[0],
    )
    .unwrap();
    let run = fixture
        .store
        .claim_task(ClaimTaskInput {
            task_id: turn.task_id,
            agent_id: fixture.agents[0].clone(),
            lease_seconds: 120,
        })
        .unwrap();
    fixture
        .store
        .link_agent_app_session(&fixture.project, &fixture.agents[0], "exact-session")
        .unwrap();
    for _ in 0..2 {
        fixture
            .store
            .record_conversation_native_event(
                "exact-session",
                1,
                "assistant_delta",
                &json!({"text":"public answer","reasoning":"PRIVATE REASONING"}),
            )
            .unwrap();
    }
    fixture
        .store
        .record_conversation_native_event(
            "unrelated-session",
            2,
            "assistant_delta",
            &json!({"text":"PRIVATE OTHER SESSION"}),
        )
        .unwrap();
    fixture
        .store
        .record_conversation_native_event(
            "exact-session",
            2,
            "turn_end",
            &json!({"cancelled":false}),
        )
        .unwrap();
    let snapshot = fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap();
    let reply = snapshot.messages.iter().find(|m| m.id == turn.id).unwrap();
    assert_eq!(reply.body, "public answer");
    assert_ne!(reply.status, "completed");
    assert_eq!(reply.run_id.as_deref(), Some(run.id.as_str()));
}

#[test]
fn restart_requires_decision_and_does_not_replay_stale_round() {
    let fixture = Fixture::new();
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    fixture.store.recover_conversations_at_startup().unwrap();
    assert_eq!(
        fixture.store.conversation_round(&round.id).unwrap().status,
        "needs_decision"
    );
    assert!(!fixture.store.begin_conversation(fixture.input()).unwrap().1);
}

#[test]
fn plan_validation_rejects_missing_scopes_traversal_cycles_and_parallel_overlap() {
    let fixture = Fixture::new();
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    let connection = fixture.store.connection.lock().unwrap();
    let valid = fixture.draft();
    assert!(validate_plan(&connection, &round, &valid).is_ok());
    for bad in [
        ".",
        "../outside",
        "C:/outside",
        "src/../outside",
        "src/*",
        ".git/config",
        "",
    ] {
        let mut draft = fixture.draft();
        draft.tasks[0].write_paths = vec![bad.into()];
        assert!(
            validate_plan(&connection, &round, &draft).is_err(),
            "accepted {bad}"
        );
    }
    let mut overlap = fixture.draft();
    let mut second = overlap.tasks[0].clone();
    second.key = "two".into();
    second.assignee_agent_id = fixture.agents[1].clone();
    overlap.tasks.push(second);
    assert!(validate_plan(&connection, &round, &overlap).is_err());
    overlap.tasks[1].depends_on = vec!["one".into()];
    assert!(validate_plan(&connection, &round, &overlap).is_ok());
    overlap.tasks[0].depends_on = vec!["two".into()];
    assert!(validate_plan(&connection, &round, &overlap).is_err());
}

#[test]
fn plan_approval_atomically_creates_scoped_existing_tasks_once() {
    let fixture = Fixture::new();
    fixture.git();
    let snapshot = fixture.run(&Replies::default());
    let round = &snapshot.rounds[0];
    let reply = snapshot.messages.last().unwrap();
    let draft = fixture.draft();
    let body = json!({"summary":draft.summary,"tasks":draft.tasks}).to_string();
    let plan = fixture
        .store
        .save_conversation_plan(&round.id, &reply.id, &body)
        .unwrap();
    let input = ConversationExecutePlanInput {
        project_id: fixture.project.clone(),
        plan_id: plan.id.clone(),
        client_request_id: "approval-1".into(),
    };
    let tasks = fixture.store.apply_conversation_plan(&input).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        fixture.store.apply_conversation_plan(&input).unwrap(),
        tasks
    );
    let snapshot = fixture.store.snapshot(&fixture.project).unwrap();
    let task = snapshot.tasks.iter().find(|t| t.id == tasks[0]).unwrap();
    assert_eq!(task.workspace_mode, WorkspaceMode::Worktree);
    assert_eq!(
        task.assignee_agent_id.as_deref(),
        Some(fixture.agents[0].as_str())
    );
    assert_eq!(
        task.verification_command.as_deref(),
        Some("node --test tests/owned.test.mjs")
    );
    fixture
        .store
        .verify_conversation_write_scope(&tasks[0], &["src/owned/file.ts".into()])
        .unwrap();
    assert!(fixture
        .store
        .verify_conversation_write_scope(&tasks[0], &["src/other.ts".into()])
        .is_err());
    let mut again = input;
    again.client_request_id = "approval-2".into();
    assert!(fixture.store.apply_conversation_plan(&again).is_err());
}

#[test]
fn old_plan_is_invalidated_by_new_human_instruction() {
    let fixture = Fixture::new();
    let snapshot = fixture.run(&Replies::default());
    let draft = fixture.draft();
    let plan = fixture
        .store
        .save_conversation_plan(
            &snapshot.rounds[0].id,
            &snapshot.messages.last().unwrap().id,
            &json!({"summary":draft.summary,"tasks":draft.tasks}).to_string(),
        )
        .unwrap();
    fixture
        .store
        .interrupt_conversation(ConversationInterruptInput {
            project_id: fixture.project.clone(),
            client_message_id: "changed-goal".into(),
            body: "Do something else".into(),
            ..Default::default()
        })
        .unwrap();
    assert!(fixture
        .store
        .apply_conversation_plan(&ConversationExecutePlanInput {
            project_id: fixture.project.clone(),
            plan_id: plan.id,
            client_request_id: "approve".into()
        })
        .unwrap_err()
        .to_string()
        .contains("替代"));
}

#[test]
fn malformed_plan_summary_is_preserved_as_real_reply_with_decision_status() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.mode = "execution_plan".into();
    let (round, _) = fixture.store.begin_conversation(input).unwrap();
    run_discussion(
        &fixture.store,
        &round.id,
        &Arc::new(AtomicBool::new(false)),
        &Replies::default(),
    )
    .unwrap();
    let snapshot = fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap();
    assert_eq!(snapshot.rounds[0].status, "needs_decision");
    assert!(snapshot.execution_plans.is_empty());
    assert!(snapshot
        .messages
        .last()
        .unwrap()
        .body
        .contains("Actual fixture reply 1"));
}

fn existing_business_run(fixture: &Fixture) -> super::super::AgentRun {
    let task = fixture
        .store
        .create_task(CreateTaskInput {
            project_id: fixture.project.clone(),
            parent_id: None,
            stage: 0,
            title: "existing business work".into(),
            description: "keep original execution owner".into(),
            acceptance_criteria: "finish business task".into(),
            verification_command: None,
            assignee_agent_id: Some(fixture.agents[0].clone()),
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::Worktree,
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
            agent_id: fixture.agents[0].clone(),
            lease_seconds: 120,
        })
        .unwrap();
    fixture
        .store
        .update_run(UpdateRunInput {
            run_id: run.id,
            status: RunStatus::Running,
            output_summary: None,
            error: None,
            session_locator: Some(json!({"businessIdentity":"original-process"})),
        })
        .unwrap()
}
struct MustNotExecute;
impl DiscussionExecutor for MustNotExecute {
    fn execute(
        &self,
        _: &MultiAgentStore,
        _: &str,
        _: &str,
        _: &str,
        _: &AtomicBool,
    ) -> Result<()> {
        panic!("discussion must not start a second runtime while its member owns a business run");
    }
}

#[test]
fn busy_member_timeout_preserves_original_business_run_and_lease() {
    let fixture = Fixture::new();
    let original = existing_business_run(&fixture);
    let (mut round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    round.deadline_at = now() + 100;
    save_round(&fixture.store.connection.lock().unwrap(), &round).unwrap();
    run_discussion(
        &fixture.store,
        &round.id,
        &Arc::new(AtomicBool::new(false)),
        &MustNotExecute,
    )
    .unwrap();
    let business = fixture.store.snapshot(&fixture.project).unwrap();
    assert_eq!(business.runs.len(), 1);
    assert_eq!(business.runs[0].id, original.id);
    assert_eq!(business.runs[0].status, RunStatus::Running);
    assert_eq!(business.runs[0].lease_expires_at, original.lease_expires_at);
    assert_eq!(business.runs[0].session_locator, original.session_locator);
    let conversation = fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap();
    assert_eq!(conversation.rounds[0].status, "needs_decision");
    assert!(conversation
        .messages
        .iter()
        .filter(|m| m.sender_type == "agent")
        .all(|m| m.run_id.is_none() && m.status == "cancelled"));
}

#[test]
fn pausing_queued_discussion_does_not_stop_or_pause_member_business_project() {
    let fixture = Fixture::new();
    let original = existing_business_run(&fixture);
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    let store = fixture.store.clone();
    let round_id = round.id.clone();
    let thread = std::thread::spawn(move || {
        run_discussion(
            &store,
            &round_id,
            &Arc::new(AtomicBool::new(false)),
            &MustNotExecute,
        )
        .unwrap()
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if fixture
            .store
            .conversation_snapshot(&fixture.project)
            .unwrap()
            .messages
            .iter()
            .any(|m| m.task_id.is_some())
        {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    ConversationSupervisor::default()
        .control(&fixture.store, &fixture.project, "pause")
        .unwrap();
    thread.join().unwrap();
    let snapshot = fixture.store.snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.project.status, ProjectStatus::Active);
    assert_eq!(snapshot.runs.len(), 1);
    assert_eq!(snapshot.runs[0].id, original.id);
    assert_eq!(snapshot.runs[0].status, RunStatus::Running);
    assert_eq!(
        fixture.store.conversation_round(&round.id).unwrap().status,
        "paused"
    );
}

#[test]
fn unicode_message_limit_uses_characters_and_keeps_full_public_messages() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.body = "汉".repeat(32_000);
    let (round, _) = fixture.store.begin_conversation(input).unwrap();
    let turn = prepare_turn(
        &fixture.store,
        &round.id,
        0,
        "opinions",
        0,
        &fixture.agents[0],
    )
    .unwrap();
    let task = fixture
        .store
        .snapshot(&fixture.project)
        .unwrap()
        .tasks
        .into_iter()
        .find(|t| t.id == turn.task_id)
        .unwrap();
    assert!(task.description.contains(&"汉".repeat(32_000)));
    assert!(bounded_text(&"汉".repeat(32_001), "消息").is_err());
}

#[test]
fn confirmed_stop_unlocks_new_round_but_never_replays_old_dispatch() {
    let fixture = Fixture::new();
    let snapshot = fixture.run(&StopDuringRun {
        unconfirmed: true,
        interrupt: false,
    });
    let run = fixture.store.snapshot(&fixture.project).unwrap().runs[0].clone();
    fixture.store.confirm_run_stopped(&run.id, true).unwrap();
    let refreshed = fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap();
    assert_eq!(refreshed.rounds[0].status, "needs_decision");
    assert_eq!(snapshot.rounds[0].id, refreshed.rounds[0].id);
    assert!(!fixture.store.begin_conversation(fixture.input()).unwrap().1);
    let mut input = fixture.input();
    input.client_message_id = "new-after-confirm".into();
    assert!(fixture.store.begin_conversation(input).unwrap().1);
    assert_eq!(
        fixture.store.snapshot(&fixture.project).unwrap().runs.len(),
        1
    );
}

#[test]
fn snapshot_does_not_redispatch_or_rewrite_unchanged_messages() {
    let fixture = Fixture::new();
    let first = fixture.run(&Replies::default());
    let sequence = fixture.store.connection.lock().unwrap().total_changes();
    std::thread::sleep(Duration::from_millis(5));
    let second = fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap();
    assert_eq!(
        serde_json::to_string(&first.messages).unwrap(),
        serde_json::to_string(&second.messages).unwrap()
    );
    assert_eq!(
        sequence,
        fixture.store.connection.lock().unwrap().total_changes()
    );
    assert_eq!(
        fixture.store.snapshot(&fixture.project).unwrap().runs.len(),
        2
    );
    assert_eq!(
        second.messages[0].client_message_id.as_deref(),
        Some("user-1")
    );
}

#[test]
fn plan_start_receipt_is_idempotent_after_success_and_requires_explicit_resume() {
    let fixture = Fixture::new();
    fixture.git();
    let snapshot = fixture.run(&Replies::default());
    let draft = fixture.draft();
    let plan = fixture
        .store
        .save_conversation_plan(
            &snapshot.rounds[0].id,
            &snapshot.messages.last().unwrap().id,
            &json!({"summary":draft.summary,"tasks":draft.tasks}).to_string(),
        )
        .unwrap();
    let input = ConversationExecutePlanInput {
        project_id: fixture.project.clone(),
        plan_id: plan.id,
        client_request_id: "start-once".into(),
    };
    fixture.store.apply_conversation_plan(&input).unwrap();
    assert!(fixture.store.claim_conversation_plan_start(&input).unwrap());
    assert!(!fixture.store.claim_conversation_plan_start(&input).unwrap());
    fixture
        .store
        .finish_conversation_plan_start(&input, Some("fixture unavailable"))
        .unwrap();
    assert!(fixture.store.claim_conversation_plan_start(&input).unwrap());
    fixture
        .store
        .finish_conversation_plan_start(&input, None)
        .unwrap();
    fixture
        .store
        .set_project_status(&fixture.project, ProjectStatus::Paused)
        .unwrap();
    assert!(
        !fixture.store.claim_conversation_plan_start(&input).unwrap(),
        "a delayed retry must not undo a later user pause"
    );
    assert_eq!(
        fixture.store.project(&fixture.project).unwrap().status,
        ProjectStatus::Paused
    );
}

#[test]
fn execution_reply_completed_does_not_claim_task_acceptance_passed() {
    let fixture = Fixture::new();
    fixture.git();
    let snapshot = fixture.run(&Replies::default());
    let mut draft = fixture.draft();
    draft.tasks[0].verification_command = None;
    let plan = fixture
        .store
        .save_conversation_plan(
            &snapshot.rounds[0].id,
            &snapshot.messages.last().unwrap().id,
            &json!({"summary":draft.summary,"tasks":draft.tasks}).to_string(),
        )
        .unwrap();
    let tasks = fixture
        .store
        .apply_conversation_plan(&ConversationExecutePlanInput {
            project_id: fixture.project.clone(),
            plan_id: plan.id,
            client_request_id: "execute-acceptance".into(),
        })
        .unwrap();
    let run = fixture
        .store
        .claim_task(ClaimTaskInput {
            task_id: tasks[0].clone(),
            agent_id: fixture.agents[0].clone(),
            lease_seconds: 120,
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
        .finalize_successful_run(&run.id, "worktree", "real task output", false, None, None)
        .unwrap();
    let conversation = fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap();
    let reply = conversation
        .messages
        .iter()
        .find(|m| m.run_id.as_deref() == Some(run.id.as_str()))
        .unwrap();
    assert_eq!(reply.body, "real task output");
    assert_eq!(reply.status, "completed");
    let project = fixture.store.snapshot(&fixture.project).unwrap();
    assert_eq!(
        project
            .tasks
            .iter()
            .find(|t| t.id == tasks[0])
            .unwrap()
            .status,
        super::super::TaskStatus::InReview
    );
    assert!(project
        .approvals
        .iter()
        .any(|a| a.run_id.as_deref() == Some(run.id.as_str()) && a.status == "pending"));
}

#[test]
fn structured_runtime_output_is_scoped_deduplicated_and_never_imports_logs() {
    let fixture = Fixture::new();
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    let turn = prepare_turn(
        &fixture.store,
        &round.id,
        0,
        "opinions",
        0,
        &fixture.agents[0],
    )
    .unwrap();
    let run = fixture
        .store
        .claim_task(ClaimTaskInput {
            task_id: turn.task_id.clone(),
            agent_id: fixture.agents[0].clone(),
            lease_seconds: 120,
        })
        .unwrap();
    fixture
        .store
        .append_runtime_event(super::super::AppendRuntimeEventInput {
            run_id: run.id.clone(),
            event_type: "output".into(),
            payload: json!({"stream":"stdout","content":"PRIVATE RAW LOG"}),
        })
        .unwrap();
    let event = fixture
        .store
        .append_runtime_event(super::super::AppendRuntimeEventInput {
            run_id: run.id.clone(),
            event_type: "assistant_delta".into(),
            payload: json!({"text":"real ","reasoning":"PRIVATE THOUGHT"}),
        })
        .unwrap();
    fixture
        .store
        .record_conversation_runtime_event(&event)
        .unwrap();
    fixture
        .store
        .append_runtime_event(super::super::AppendRuntimeEventInput {
            run_id: run.id.clone(),
            event_type: "output".into(),
            payload: json!({"stream":"assistant","content":"real final answer"}),
        })
        .unwrap();
    let snapshot = fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap();
    let reply = snapshot.messages.iter().find(|m| m.id == turn.id).unwrap();
    assert_eq!(reply.body, "real final answer");
    assert_eq!(reply.run_id.as_deref(), Some(run.id.as_str()));
    assert_ne!(reply.status, "completed");
}

#[test]
fn pi_public_reply_accepts_only_final_assistant_text() {
    let raw=[
        json!({"type":"message_end","message":{"role":"user","content":[{"type":"text","text":"PRIVATE USER"}]}}),
        json!({"type":"message_update","assistantMessageEvent":{"type":"text_delta","delta":"partial"}}),
        json!({"type":"message_end","message":{"role":"assistant","stopReason":"stop","content":[{"type":"thinking","thinking":"PRIVATE THOUGHT"},{"type":"text","text":"public answer"}]}}),
        json!({"type":"agent_end","messages":[{"role":"user","content":"PRIVATE USER"}]}),
    ].iter().map(Value::to_string).collect::<Vec<_>>().join("\n");
    assert_eq!(
        super::super::executor::conversation_pi_output(&raw).unwrap(),
        "public answer"
    );
    assert!(super::super::executor::conversation_pi_output("terminal says success").is_err());
    assert!(super::super::executor::conversation_pi_output(&json!({"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"provider unavailable","content":[{"type":"text","text":"partial"}]}}).to_string()).is_err());
}

#[test]
fn real_git_scope_check_rejects_committed_and_untracked_out_of_scope_changes() {
    let fixture = Fixture::new();
    fixture.git();
    let project = fixture.root.join("project");
    let baseline = std::process::Command::new("git")
        .arg("-C")
        .arg(&project)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    let baseline = String::from_utf8(baseline.stdout).unwrap();
    let snapshot = fixture.run(&Replies::default());
    let draft = fixture.draft();
    let plan = fixture
        .store
        .save_conversation_plan(
            &snapshot.rounds[0].id,
            &snapshot.messages.last().unwrap().id,
            &json!({"summary":draft.summary,"tasks":draft.tasks}).to_string(),
        )
        .unwrap();
    let tasks = fixture
        .store
        .apply_conversation_plan(&ConversationExecutePlanInput {
            project_id: fixture.project.clone(),
            plan_id: plan.id,
            client_request_id: "scope-check".into(),
        })
        .unwrap();
    std::fs::create_dir_all(project.join("src/owned")).unwrap();
    std::fs::write(project.join("src/owned/ok.ts"), "allowed").unwrap();
    super::super::executor::verify_conversation_workspace_scope(
        &fixture.store,
        &tasks[0],
        &project,
        Some(baseline.trim()),
    )
    .unwrap();
    std::fs::write(project.join("outside.ts"), "out of scope").unwrap();
    assert!(super::super::executor::verify_conversation_workspace_scope(
        &fixture.store,
        &tasks[0],
        &project,
        Some(baseline.trim())
    )
    .is_err());
    for args in [
        vec!["add", "-A"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@local",
            "commit",
            "-m",
            "fixture change",
        ],
    ] {
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(&project)
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    assert!(
        super::super::executor::verify_conversation_workspace_scope(
            &fixture.store,
            &tasks[0],
            &project,
            Some(baseline.trim())
        )
        .is_err(),
        "committing early must not evade the scope check"
    );
}

fn cross_runtime_conversation_plan() -> (Fixture, ConversationRound, PlanDraft) {
    let mut fixture = Fixture::new();
    fixture
        .store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE ma_agents SET runtime_kind='claude_code' WHERE id=?1",
            [&fixture.agents[1]],
        )
        .unwrap();
    for (name, runtime_kind, role) in [
        (
            "review-codex-work",
            AgentRuntimeKind::ClaudeCode,
            AgentRole::Reviewer,
        ),
        (
            "review-claude-work",
            AgentRuntimeKind::Codex,
            AgentRole::Reviewer,
        ),
        (
            "integrate-reviewed-work",
            AgentRuntimeKind::Codex,
            AgentRole::Integrator,
        ),
    ] {
        let agent = fixture
            .store
            .create_agent(CreateAgentInput {
                avatar_id: None,
                project_id: fixture.project.clone(),
                name: name.into(),
                runtime_kind,
                role,
                description: "explicit review graph fixture".into(),
                persona: Default::default(),
                persona_custom: String::new(),
                model: None,
                permission_profile: PermissionProfile::Default,
                max_concurrency: 1,
            })
            .unwrap();
        fixture.agents.push(agent.id);
    }
    let mut input = fixture.input();
    input.mode = "execution_plan".into();
    let (round, _) = fixture.store.begin_conversation(input).unwrap();
    let mut plan = fixture.draft();
    let first = plan.tasks[0].clone();
    let mut second = first.clone();
    second.key = "two".into();
    second.title = "second implementation".into();
    second.assignee_agent_id = fixture.agents[1].clone();
    second.write_paths = vec!["src/other".into()];
    let mut review_first = first.clone();
    review_first.key = "review-one".into();
    review_first.title = "Review first implementation".into();
    review_first.assignee_agent_id = fixture.agents[2].clone();
    review_first.workspace_mode = WorkspaceMode::ReadOnly;
    review_first.write_paths.clear();
    review_first.verification_command = None;
    review_first.depends_on = vec![first.key.clone()];
    let mut review_second = review_first.clone();
    review_second.key = "review-two".into();
    review_second.title = "Review second implementation".into();
    review_second.assignee_agent_id = fixture.agents[3].clone();
    review_second.depends_on = vec![second.key.clone()];
    let mut integration = first;
    integration.key = "integrate".into();
    integration.title = "integrate reviewed work".into();
    integration.assignee_agent_id = fixture.agents[4].clone();
    integration.write_paths = vec!["src/owned".into(), "src/other".into()];
    integration.depends_on = vec![review_first.key.clone(), review_second.key.clone()];
    plan.tasks
        .extend([second, review_first, review_second, integration]);
    (fixture, round, plan)
}

#[test]
fn conversation_cross_runtime_missing_reviews_rejects_without_persisting_plan() {
    let (fixture, round, mut plan) = cross_runtime_conversation_plan();
    plan.tasks.truncate(2);
    let error = fixture
        .store
        .save_conversation_plan(
            &round.id,
            &round.user_message_id,
            &json!({"summary":plan.summary,"tasks":plan.tasks}).to_string(),
        )
        .unwrap_err();
    assert!(error.to_string().contains("独立只读 Review"), "{error}");
    let snapshot = fixture
        .store
        .conversation_snapshot(&fixture.project)
        .unwrap();
    assert!(snapshot.execution_plans.is_empty());
    assert!(fixture
        .store
        .snapshot(&fixture.project)
        .unwrap()
        .tasks
        .is_empty());
}

#[test]
fn conversation_cross_runtime_rejects_same_runtime_or_writable_review() {
    let (fixture, round, mut plan) = cross_runtime_conversation_plan();
    plan.tasks[2].assignee_agent_id = fixture.agents[3].clone();
    let connection = fixture.store.connection.lock().unwrap();
    assert!(validate_plan(&connection, &round, &plan)
        .unwrap_err()
        .to_string()
        .contains("Review"));
    plan.tasks[2].assignee_agent_id = fixture.agents[2].clone();
    plan.tasks[2].workspace_mode = WorkspaceMode::Worktree;
    plan.tasks[2].write_paths = vec!["review-notes".into()];
    assert!(validate_plan(&connection, &round, &plan)
        .unwrap_err()
        .to_string()
        .contains("Review"));
}

#[test]
fn conversation_cross_runtime_requires_direct_review_dependency_and_complete_integration() {
    let (fixture, round, mut plan) = cross_runtime_conversation_plan();
    // Keep write scopes independent so each failing assertion exercises the shared graph policy.
    plan.tasks[4].write_paths = vec!["integration-only".into()];
    let connection = fixture.store.connection.lock().unwrap();
    plan.tasks[2].depends_on.clear();
    assert!(validate_plan(&connection, &round, &plan)
        .unwrap_err()
        .to_string()
        .contains("Review"));
    plan.tasks[2].depends_on = vec!["one".into()];
    plan.tasks[4].depends_on = vec!["review-one".into()];
    assert!(validate_plan(&connection, &round, &plan)
        .unwrap_err()
        .to_string()
        .contains("依赖全部"));
    plan.tasks.pop();
    assert!(validate_plan(&connection, &round, &plan)
        .unwrap_err()
        .to_string()
        .contains("最终集成"));
}

#[test]
fn conversation_cross_runtime_integrator_must_use_worktree() {
    let (fixture, round, mut plan) = cross_runtime_conversation_plan();
    plan.tasks[4].workspace_mode = WorkspaceMode::ReadOnly;
    plan.tasks[4].write_paths.clear();
    let error =
        validate_plan(&fixture.store.connection.lock().unwrap(), &round, &plan).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("最终集成任务必须使用独立 Worktree"),
        "{error}"
    );
}

#[test]
fn conversation_cross_runtime_valid_review_graph_applies_all_tasks_and_dependencies() {
    let (fixture, round, plan) = cross_runtime_conversation_plan();
    fixture.git();
    fixture
        .store
        .finish_conversation(&round.id, "completed", None)
        .unwrap();
    let stored = fixture
        .store
        .save_conversation_plan(
            &round.id,
            &round.user_message_id,
            &json!({"summary":plan.summary,"tasks":plan.tasks}).to_string(),
        )
        .unwrap();
    let task_ids = fixture
        .store
        .apply_conversation_plan(&ConversationExecutePlanInput {
            project_id: fixture.project.clone(),
            plan_id: stored.id,
            client_request_id: "approved-cross-runtime".into(),
        })
        .unwrap();
    assert_eq!(task_ids.len(), 5);
    let snapshot = fixture.store.snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.dependencies.len(), 4);
    for (task_index, dependency_index) in [(2, 0), (3, 1), (4, 2), (4, 3)] {
        assert!(snapshot
            .dependencies
            .iter()
            .any(|dependency| dependency.task_id == task_ids[task_index]
                && dependency.depends_on_task_id == task_ids[dependency_index]));
    }
    assert_eq!(
        snapshot
            .tasks
            .iter()
            .filter(|task| task.workspace_mode == WorkspaceMode::ReadOnly)
            .count(),
        2
    );
    assert!(
        snapshot.runs.is_empty(),
        "saving an approved graph does not fabricate execution"
    );
}

#[test]
fn conversation_cross_runtime_rechecks_changed_reviewer_at_approval() {
    let (fixture, round, plan) = cross_runtime_conversation_plan();
    fixture.git();
    fixture
        .store
        .finish_conversation(&round.id, "completed", None)
        .unwrap();
    let stored = fixture
        .store
        .save_conversation_plan(
            &round.id,
            &round.user_message_id,
            &json!({"summary":plan.summary,"tasks":plan.tasks}).to_string(),
        )
        .unwrap();
    fixture
        .store
        .connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE ma_agents SET role='worker' WHERE id=?1",
            [&fixture.agents[2]],
        )
        .unwrap();
    let error = fixture
        .store
        .apply_conversation_plan(&ConversationExecutePlanInput {
            project_id: fixture.project.clone(),
            plan_id: stored.id,
            client_request_id: "stale-review-role".into(),
        })
        .unwrap_err();
    assert!(error.to_string().contains("Review"), "{error}");
    assert!(fixture
        .store
        .snapshot(&fixture.project)
        .unwrap()
        .tasks
        .is_empty());
    assert_eq!(
        fixture
            .store
            .conversation_snapshot(&fixture.project)
            .unwrap()
            .execution_plans[0]
            .status,
        "pending"
    );
}

#[test]
fn conversation_plan_prompt_names_actual_roles_and_requires_the_shared_review_graph() {
    let (fixture, round, _) = cross_runtime_conversation_plan();
    let prompt = discussion_prompt(
        &fixture.store.connection.lock().unwrap(),
        &round,
        "summary",
        1,
        &fixture.agents[0],
    )
    .unwrap();
    assert!(prompt.contains("runtime=claude_code | role=reviewer"));
    assert!(prompt.contains("role=integrator"));
    assert!(prompt.contains("不同于被审查"));
    assert!(prompt.contains("dependsOn 必须覆盖全部 Review"));
    assert!(prompt.contains("仅设置 reviewerAgentId 不构成独立 Review 任务"));
    assert!(prompt.contains("真实成员不足"));
}
