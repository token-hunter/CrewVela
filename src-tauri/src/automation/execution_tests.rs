use super::*;
use crate::{
    control::{QueuedMessage, SessionActionQueue},
    multi_agent::MultiAgentStore,
    sessions::{NewRuntimeSession, SessionManager},
};

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Fixture {
    root: PathBuf,
    manager: AutomationManager,
    sessions: SessionManager,
    queue: SessionActionQueue,
    store: MultiAgentStore,
    id: String,
    session: String,
    _cleanup: Cleanup,
}

#[test]
fn conversation_template_schedule_skips_paused_projects_and_reconciles_the_exact_durable_round() {
    use crate::multi_agent::{CreateProjectInput, ProjectStatus};
    let f = Fixture::new();
    let project = f.store.create_project(CreateProjectInput {
        name: "Schedule test".into(), goal: "Scoped review".into(), root_path: f.root.display().to_string(), default_branch: "main".into(),
    }).unwrap();
    let member = f.store.create_agent(serde_json::from_value(serde_json::json!({
        "projectId": project.id, "name": "Reader", "runtimeKind": "claude_code", "role": "reviewer"
    })).unwrap()).unwrap();
    let topic = f.store.conversation_snapshot(&project.id).unwrap().conversation.id;
    let request_id = uuid::Uuid::new_v4().to_string();
    let draft: NewAutomation = serde_json::from_value(serde_json::json!({
        "name":"Review", "trigger":{"kind":"interval","minutes":1}, "overlapPolicy":"skip", "maxRuns":null,
        "maxConsecutiveFailures":3, "action":{"kind":"conversation_template","projectId":project.id,
            "conversationId":topic,"agentId":member.id,"templateId":"pr_review","body":"Read the supplied patch.",
            "creationId": request_id}
    })).unwrap();
    let item = f.manager.create(draft.clone(), 1).unwrap();
    assert_eq!(f.manager.create(draft, 2).unwrap().id, item.id);
    assert_eq!(item.id, request_id);
    let due = |at| f.manager.collect_due_for_projects(at, &Default::default(), Some(&f.store)).unwrap();
    f.store.set_project_status(&project.id, ProjectStatus::Paused).unwrap();
    assert!(due(60_001).is_empty());
    let held = f.manager.list().into_iter().find(|a| a.id == item.id).unwrap();
    assert_eq!(held.run_count, 0);
    assert_eq!(held.consecutive_failures, 0);
    assert_eq!(held.recent_runs[0].stage.as_deref(), Some("skipped"));
    f.store.set_project_status(&project.id, ProjectStatus::Active).unwrap();
    let (id, run_id, action) = due(120_001).pop().unwrap();
    assert_eq!(id, item.id);
    let AutomationAction::ConversationTemplate(action) = action else { panic!("unexpected action") };
    action.validate(&f.store).unwrap();
    let input = f.manager.reserve_template(&action, &run_id).unwrap();
    assert!(f.manager.reserve_template(&action, &run_id).is_err());
    assert!(!input.native_capabilities);
    assert_eq!(input.expected_conversation_id.as_deref(), Some(topic.as_str()));
    assert_eq!(input.participant_agent_ids, [member.id]);
    let (round, _) = f.store.begin_conversation(input).unwrap();
    f.manager.finish_dispatch(&id, &run_id, true, "accepted".into(), 120_002).unwrap();
    let reopened = AutomationManager::open_at(f.root.join("automations.json")).unwrap();
    let status = || reopened.list().into_iter().find(|a| a.id == id).unwrap().recent_runs.last().unwrap().stage.clone();
    assert_eq!(status().as_deref(), Some("unconfirmed"));
    reopened.reconcile(&f.queue, &f.sessions, &f.store, 120_003).unwrap();
    assert_eq!(status().as_deref(), Some("queued"));
    f.store.finish_conversation(&round.id, "completed", None).unwrap();
    reopened.reconcile(&f.queue, &f.sessions, &f.store, 120_004).unwrap();
    assert_eq!(status().as_deref(), Some("completed"));
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("automation-execution-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let manager = AutomationManager::open_at(root.join("automations.json")).unwrap();
        let sessions =
            SessionManager::open_at(root.join("sessions.json"), root.join("events")).unwrap();
        let queue =
            SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox")).unwrap();
        let store = MultiAgentStore::open_at(root.join("store.db")).unwrap();
        let session = sessions
            .create_runtime(NewRuntimeSession {
                runtime_kind: "claude_code".into(),
                cwd: root.display().to_string(),
                project_id: None,
            })
            .unwrap()
            .id;
        let id = manager
            .create(
                NewAutomation {
                    name: "daily".into(),
                    trigger: AutomationTrigger::Daily { hour: 9, minute: 0 },
                    action: AutomationAction::Broadcast {
                        body: "exact".into(),
                    },
                    max_runs: None,
                    max_consecutive_failures: Some(3),
                    overlap_policy: OverlapPolicy::Skip,
                },
                1,
            )
            .unwrap()
            .id;
        let _cleanup = Cleanup(root.clone());
        Self {
            root,
            manager,
            sessions,
            queue,
            store,
            id,
            session,
            _cleanup,
        }
    }
    fn enqueue(&self) -> (String, String) {
        let (run, _) = self
            .manager
            .begin_manual(&self.id, &uuid::Uuid::new_v4().to_string(), None, 2)
            .unwrap()
            .unwrap();
        let mut message = QueuedMessage::new("exact".into(), None);
        message.id = format!("automation-{run}-message");
        self.manager
            .register_target(
                &run,
                AutomationTarget {
                    session_id: self.session.clone(),
                    message_id: message.id.clone(),
                    project_id: None,
                    task_id: None,
                    run_id: None,
                    stage: "queued".into(),
                    detail: "queued".into(),
                },
            )
            .unwrap();
        self.queue
            .enqueue_messages_durable(vec![(self.session.clone(), message.clone())])
            .unwrap();
        self.manager
            .finish_dispatch(&self.id, &run, true, "queued".into(), 3)
            .unwrap();
        (run, message.id)
    }
    fn stage(&self) -> String {
        self.manager.list()[0]
            .recent_runs
            .last()
            .unwrap()
            .stage
            .clone()
            .unwrap()
    }
    fn hook(&self, name: &str, prompt: &str, at: Option<u64>) {
        let record = self.sessions.get(&self.session).unwrap();
        let at = at.unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        });
        self.sessions.apply_hook_value(&serde_json::json!({crate::brand::APP_SESSION:self.session,"session_id":record.claude_session_id,"hook_event_name":name,"prompt":prompt,"input_prompt":prompt,"_crewvela_queued_at":at})).unwrap();
    }
    fn reconcile(&self) {
        self.manager
            .reconcile(&self.queue, &self.sessions, &self.store, 10)
            .unwrap();
    }
}

#[test]
fn automation_manual_receipt_is_durable_and_does_not_consume_schedule() {
    let f = Fixture::new();
    let original = f.manager.list()[0].clone();
    let request = uuid::Uuid::new_v4().to_string();
    let (run, _) = f
        .manager
        .begin_manual(&f.id, &request, None, 2)
        .unwrap()
        .unwrap();
    assert!(f
        .manager
        .begin_manual(&f.id, &request, None, 3)
        .unwrap()
        .is_none());
    let current = f.manager.list()[0].clone();
    assert_eq!(current.run_count, 0);
    assert_eq!(current.next_run_at, original.next_run_at);
    assert!(current.enabled);
    f.manager
        .finish_dispatch(&f.id, &run, false, "auth failed".into(), 4)
        .unwrap();
    let reopened = AutomationManager::open_at(f.root.join("automations.json")).unwrap();
    assert!(reopened
        .begin_manual(&f.id, &request, None, 5)
        .unwrap()
        .is_none());
    assert_eq!(reopened.list()[0].recent_runs.len(), 1);
}

#[test]
fn automation_queue_receipt_is_not_completion_and_stop_preserves_other_messages() {
    let f = Fixture::new();
    let (run, _) = f.enqueue();
    f.queue
        .enqueue_messages_durable(vec![(
            f.session.clone(),
            QueuedMessage::new("other".into(), None),
        )])
        .unwrap();
    f.reconcile();
    assert_eq!(f.stage(), "queued");
    f.manager.cancel_queued_run(&f.id, &run, &f.queue).unwrap();
    assert_eq!(f.stage(), "cancelled");
    assert_eq!(f.queue.peek_next(&f.session).unwrap().body, "other");
    assert!(f.manager.list()[0].enabled);
}

#[test]
fn automation_recovery_holds_mail_and_never_replays_uncertain_delivery() {
    let f = Fixture::new();
    let (run, _) = f.enqueue();
    let reopened =
        SessionActionQueue::open_at(f.root.join("mailbox.json"), f.root.join("outbox")).unwrap();
    assert!(reopened.peek_next(&f.session).is_some());
    assert!(reopened.peek_deliverable(&f.session).is_none());
    let manager = AutomationManager::open_at(f.root.join("automations.json")).unwrap();
    manager
        .reconcile(&reopened, &f.sessions, &f.store, 10)
        .unwrap();
    assert_eq!(
        manager.list()[0].recent_runs[0].stage.as_deref(),
        Some("unconfirmed")
    );
    assert!(manager
        .begin_manual(&f.id, &uuid::Uuid::new_v4().to_string(), Some(&run), 11)
        .is_err());
    manager.cancel_queued_run(&f.id, &run, &reopened).unwrap();
}

#[test]
fn automation_completion_requires_fresh_matching_start_and_stop() {
    let f = Fixture::new();
    let (_, message) = f.enqueue();
    f.sessions
        .bind_automation_message(&f.session, &message, "exact");
    f.hook("Stop", "exact", None);
    f.reconcile();
    assert_ne!(f.stage(), "completed");
    f.hook("UserPromptSubmit", "exact", Some(1));
    f.hook("Stop", "exact", None);
    f.reconcile();
    assert_ne!(
        f.stage(),
        "completed",
        "old input event is not current acceptance"
    );
    f.hook("UserPromptSubmit", "exact", None);
    f.reconcile();
    assert_eq!(f.stage(), "running");
    f.hook("Stop", "other", None);
    f.reconcile();
    assert_ne!(f.stage(), "completed");
    f.hook("Stop", "exact", None);
    f.reconcile();
    assert_eq!(f.stage(), "completed");
    let item = f.manager.list()[0].clone();
    assert!(item.enabled);
    assert!(item.next_run_at.is_some());
    assert_eq!(item.run_count, 0);
}

#[test]
fn automation_manual_takeover_does_not_claim_someone_elses_reply() {
    let f = Fixture::new();
    let (_, message) = f.enqueue();
    f.sessions
        .bind_automation_message(&f.session, &message, "exact");
    f.hook("UserPromptSubmit", "exact", None);
    f.hook("UserPromptSubmit", "manual task", None);
    f.hook("Stop", "manual task", None);
    f.reconcile();
    assert_eq!(f.stage(), "unconfirmed");
}

#[test]
fn automation_stop_refuses_entire_batch_when_one_message_already_delivered() {
    let f = Fixture::new();
    let (_, message) = f.enqueue();
    assert!(f.queue.commit_if_id_matches(&f.session, &message));
    f.queue
        .enqueue_messages_durable(vec![(
            f.session.clone(),
            QueuedMessage::new("other".into(), None),
        )])
        .unwrap();
    assert!(f
        .queue
        .cancel_automation_messages(&[(f.session.clone(), message)])
        .is_err());
    assert_eq!(f.queue.peek_next(&f.session).unwrap().body, "other");
}

#[test]
fn automation_dispatch_success_does_not_reset_failure_streak() {
    let f = Fixture::new();
    f.manager
        .report_result(&f.id, false, "failure".into(), 1)
        .unwrap();
    let (run, _) = f
        .manager
        .begin_manual(&f.id, &uuid::Uuid::new_v4().to_string(), None, 2)
        .unwrap()
        .unwrap();
    f.manager
        .finish_dispatch(&f.id, &run, true, "queued".into(), 3)
        .unwrap();
    assert_eq!(f.manager.list()[0].consecutive_failures, 1);
}

#[test]
fn automation_project_result_uses_exact_task_run_and_settles_only_once() {
    use crate::multi_agent::*;
    let f = Fixture::new();
    let project = f
        .store
        .create_project(CreateProjectInput {
            name: "automation".into(),
            goal: "exact evidence".into(),
            root_path: f.root.display().to_string(),
            default_branch: "main".into(),
        })
        .unwrap();
    let agent=f.store.create_agent(serde_json::from_value(serde_json::json!({"projectId":project.id,"name":"worker","runtimeKind":"codex","role":"worker"})).unwrap()).unwrap();
    let (execution, _) = f
        .manager
        .begin_manual(&f.id, &uuid::Uuid::new_v4().to_string(), None, 1)
        .unwrap()
        .unwrap();
    let message = format!("automation-{execution}-message");
    let task = format!("cli-message-{message}");
    f.manager
        .register_target(
            &execution,
            AutomationTarget {
                session_id: f.session.clone(),
                message_id: message.clone(),
                project_id: Some(project.id.clone()),
                task_id: Some(task.clone()),
                run_id: None,
                stage: "queued".into(),
                detail: "queued".into(),
            },
        )
        .unwrap();
    f.manager
        .finish_dispatch(&f.id, &execution, true, "queued".into(), 2)
        .unwrap();
    f.store
        .enqueue_cli_message_task(&project.id, &agent.id, &message, "exact")
        .unwrap();
    let run = f
        .store
        .claim_task(ClaimTaskInput {
            task_id: task.clone(),
            agent_id: agent.id.clone(),
            lease_seconds: 60,
        })
        .unwrap();
    f.store
        .update_run(UpdateRunInput {
            run_id: run.id.clone(),
            status: RunStatus::Running,
            output_summary: None,
            error: None,
            session_locator: None,
        })
        .unwrap();
    f.reconcile();
    assert_eq!(f.stage(), "running");
    assert_eq!(
        f.manager.list()[0].recent_runs[0].targets[0]
            .run_id
            .as_ref(),
        Some(&run.id)
    );
    f.store
        .finalize_successful_run(
            &run.id,
            &f.root.display().to_string(),
            "exact result",
            true,
            None,
            None,
        )
        .unwrap();
    f.reconcile();
    assert_eq!(f.stage(), "completed");
    assert!(f.manager.list()[0].enabled);
    f.reconcile();
    assert_eq!(f.manager.list()[0].recent_runs.len(), 1);
}

#[test]
fn automation_acknowledgement_releases_unknown_without_claiming_success_or_replaying() {
    let f = Fixture::new();
    let (run, _) = f.enqueue();
    let manager = AutomationManager::open_at(f.root.join("automations.json")).unwrap();
    manager
        .acknowledge_unknown(&f.id, &run, &f.queue, &f.store)
        .unwrap();
    assert_eq!(
        manager.list()[0].recent_runs[0].stage.as_deref(),
        Some("acknowledged")
    );
    assert!(f.queue.peek_next(&f.session).is_none());
    assert_eq!(manager.list()[0].run_count, 0);
    assert!(
        manager
            .begin_manual(&f.id, &uuid::Uuid::new_v4().to_string(), Some(&run), 10)
            .is_err(),
        "acknowledgement is not proof of failure"
    );
    assert!(manager
        .begin_manual(&f.id, &uuid::Uuid::new_v4().to_string(), None, 10)
        .unwrap()
        .is_some());
}

#[test]
fn automation_missing_cli_completion_becomes_attention_instead_of_fake_success() {
    let f = Fixture::new();
    let (_, message) = f.enqueue();
    f.sessions
        .bind_automation_message(&f.session, &message, "exact");
    f.manager
        .reconcile(&f.queue, &f.sessions, &f.store, 60_003)
        .unwrap();
    assert_eq!(f.stage(), "unconfirmed");
    f.hook("UserPromptSubmit", "exact", None);
    f.hook("Stop", "exact", None);
    f.reconcile();
    assert_eq!(
        f.stage(),
        "completed",
        "late exact result still resolves uncertainty"
    );
}

#[test]
fn automation_v3_legacy_failures_stay_failed_and_dispatch_receipts_stay_unknown() {
    let f = Fixture::new();
    let mut item = serde_json::to_value(f.manager.list()[0].clone()).unwrap();
    item["recentRuns"] = serde_json::json!([{ "at":1,"ok":false,"detail":"old failure" },{"at":2,"ok":true,"stage":"queued","detail":"old dispatch"}]);
    let path = f.root.join("legacy.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({"version":3,"items":[item]})).unwrap(),
    )
    .unwrap();
    let manager = AutomationManager::open_at(path).unwrap();
    let item = manager.list().remove(0);
    assert_eq!(item.recent_runs[0].stage.as_deref(), Some("failed"));
    assert_eq!(item.recent_runs[1].stage.as_deref(), Some("unconfirmed"));
    assert!(!item.recent_runs[1].id.is_empty());
}
