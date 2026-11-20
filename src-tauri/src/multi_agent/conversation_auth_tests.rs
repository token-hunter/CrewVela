use super::*;
use crate::multi_agent::conversation::auth::{CodexFailedTurn, CodexRetryInput, LOGIN_REPLY};

struct RepliesAfterLogin {
    ready: bool,
    executed: Mutex<Vec<String>>,
}
impl DiscussionExecutor for RepliesAfterLogin {
    fn codex_ready(&self, _: &MultiAgentStore, _: &ConversationRound, _: &str) -> Result<bool> {
        Ok(self.ready)
    }
    fn execute(
        &self,
        store: &MultiAgentStore,
        _: &str,
        run: &str,
        agent: &str,
        _: &AtomicBool,
    ) -> Result<()> {
        self.executed.lock().unwrap().push(agent.into());
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
            &format!("reply by {agent}"),
            false,
            None,
            None,
        )
    }
}

#[test]
fn mixed_chat_retries_only_preflight_failures_on_the_original_task_after_restart() -> Result<()> {
    for dispatch in [None, Some(ConversationChatDispatch::Parallel)] {
        let fixture = Fixture::new();
        fixture.store.connection.lock().unwrap().execute(
            "UPDATE ma_agents SET runtime_kind='claude_code' WHERE id=?1",
            [&fixture.agents[1]],
        )?;
        fixture
            .store
            .set_project_status(&fixture.project, ProjectStatus::Completed)?;
        let mut input = fixture.input();
        input.native_capabilities = true;
        input.chat_dispatch = dispatch;
        let blocked = RepliesAfterLogin {
            ready: false,
            executed: Mutex::new(Vec::new()),
        };
        let snapshot = fixture.run_input(input, &blocked);
        assert_eq!(
            blocked.executed.into_inner().unwrap(),
            [fixture.agents[1].clone()]
        );
        let turn = snapshot
            .messages
            .iter()
            .find(|m| m.kind == LOGIN_REPLY)
            .unwrap();
        assert_eq!(turn.agent_id.as_ref(), Some(&fixture.agents[0]));
        assert!(turn.run_id.is_none());
        assert_eq!(snapshot.rounds[0].status, "failed");
        let good = snapshot
            .messages
            .iter()
            .find(|m| m.agent_id.as_ref() == Some(&fixture.agents[1]))
            .unwrap();
        let request = CodexRetryInput {
            project_id: fixture.project.clone(),
            expected_conversation_id: snapshot.conversation.id.clone(),
            round_id: snapshot.rounds[0].id.clone(),
            turns: vec![
                CodexFailedTurn {
                    message_id: turn.id.clone(),
                    run_id: None,
                },
                CodexFailedTurn {
                    message_id: turn.id.clone(),
                    run_id: None,
                },
                CodexFailedTurn {
                    message_id: good.id.clone(),
                    run_id: good.run_id.clone(),
                },
            ],
        };
        let reopened = MultiAgentStore::open_at(fixture.root.join("store.db"))?;
        let (round, turns) = reopened.prepare_codex_retries(&request)?.unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(Some(&turns[0].task_id), turn.task_id.as_ref());
        assert!(reopened.prepare_codex_retries(&request).is_err());
        let ready = RepliesAfterLogin {
            ready: true,
            executed: Mutex::new(Vec::new()),
        };
        auth::run_retries(&reopened, &round, &turns, &Arc::new(false.into()), &ready)?;
        let conversation = reopened.conversation_snapshot(&fixture.project)?;
        assert_eq!(conversation.rounds[0].status, "completed");
        assert_eq!(
            conversation
                .messages
                .iter()
                .filter(|m| m.sender_type == "user")
                .count(),
            1
        );
        assert_eq!(
            ready.executed.into_inner().unwrap(),
            [fixture.agents[0].clone()]
        );
        assert_eq!(
            conversation
                .messages
                .iter()
                .find(|m| m.id == good.id)
                .unwrap()
                .run_id,
            good.run_id
        );
        assert!(reopened.prepare_codex_retries(&request)?.is_none());
        assert_eq!(
            reopened.project(&fixture.project)?.status,
            ProjectStatus::Completed
        );
    }
    Ok(())
}

#[test]
fn persisted_pre_session_acp_failure_recovers_without_replaying_bound_or_personal_threads(
) -> Result<()> {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.participant_agent_ids.truncate(1);
    let (round, _) = fixture.store.begin_conversation(input)?;
    let turn = prepare_turn(&fixture.store, &round.id, 0, "chat", 0, &fixture.agents[0])?;
    let run = fixture.store.claim_task(ClaimTaskInput {
        task_id: turn.task_id.clone(),
        agent_id: turn.agent_id.clone(),
        lease_seconds: 120,
    })?;
    let error = "Codex 专用目录认证失败。群聊 ACP 未就绪，未更换权限或切换供应商：ACP 请求失败: {\"code\":-32000,\"message\":\"Authentication required\"}";
    let locator =
        json!({"adapter":"codex","transport":"acp","cwd":fixture.root,"codexStorage":"crewvela"});
    fixture
        .store
        .fail_run_permanently(&run.id, error, Some(locator.clone()))?;
    fixture
        .store
        .finish_conversation(&round.id, "failed", Some(error.into()))?;
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db"))?;
    let snapshot = reopened.conversation_snapshot(&fixture.project)?;
    let failed = snapshot.messages.iter().find(|m| m.id == turn.id).unwrap();
    assert_eq!(failed.kind, LOGIN_REPLY);
    let mut retry = CodexRetryInput {
        project_id: fixture.project.clone(),
        expected_conversation_id: snapshot.conversation.id,
        round_id: round.id,
        turns: vec![CodexFailedTurn {
            message_id: turn.id.clone(),
            run_id: None,
        }],
    };
    assert!(
        reopened.prepare_codex_retries(&retry)?.is_none(),
        "obsolete attempt"
    );
    retry.turns[0].run_id = Some(run.id.clone());
    for unsafe_locator in [
        json!({"adapter":"codex"}),
        json!({"codexStorage":"personal"}),
        json!({"codexStorage":"crewvela","threadId":"previous"}),
        json!({"codexStorage":"crewvela","sessionId":"previous"}),
        json!({"codexStorage":"crewvela","stopUnverified":true}),
    ] {
        reopened.connection.lock().unwrap().execute(
            "UPDATE ma_runs SET session_locator_json=?2 WHERE id=?1",
            params![run.id, unsafe_locator.to_string()],
        )?;
        assert!(reopened.prepare_codex_retries(&retry)?.is_none());
    }
    reopened.connection.lock().unwrap().execute(
        "UPDATE ma_runs SET session_locator_json=?2 WHERE id=?1",
        params![run.id, locator.to_string()],
    )?;
    let (round, turns) = reopened.prepare_codex_retries(&retry)?.unwrap();
    auth::run_retries(
        &reopened,
        &round,
        &turns,
        &Arc::new(false.into()),
        &RepliesAfterLogin {
            ready: true,
            executed: Mutex::new(Vec::new()),
        },
    )?;
    let runs = reopened.snapshot(&fixture.project)?.runs;
    assert_eq!(runs.len(), 2);
    assert!(runs
        .iter()
        .any(|old| old.id == run.id && old.error.as_deref() == Some(error)));
    assert!(runs.iter().any(|new| new.attempt == 2
        && new.status == RunStatus::Succeeded
        && new.task_id == turn.task_id));
    assert!(reopened.prepare_codex_retries(&retry)?.is_none());
    Ok(())
}
