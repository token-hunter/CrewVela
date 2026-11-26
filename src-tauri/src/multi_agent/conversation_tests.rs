use super::*;
use crate::multi_agent::{
    AgentRole, CreateAgentInput, CreateProjectInput, CreateTaskInput, PermissionProfile,
    ProjectStatus, RunStatus, SendCollaborationMessageInput, TaskStatus, UpdateRunInput,
};

#[path = "conversation_chat_tests.rs"]
mod natural_chat;
#[path = "conversation_live_tests.rs"]
mod live_chat;
#[path = "conversation_views_tests.rs"]
mod work_views;
#[path = "conversation_policy_tests.rs"]
mod research_policy;
#[path = "conversation_activity_tests.rs"]
mod activity;
#[path = "conversation_auth_tests.rs"]
mod codex_login;
#[path = "managed_native_tests.rs"]
mod managed_native;
#[path = "managed_small_team_tests.rs"]
mod managed_small_team;
#[path = "managed_pi_native_tests.rs"]
mod managed_pi_native;
#[path = "managed_acp_native_tests.rs"]
mod managed_acp_native;
#[path = "managed_unified_live_tests.rs"]
mod managed_unified_live;

struct Fixture {
    store: MultiAgentStore,
    root: std::path::PathBuf,
    project: String,
    agents: Vec<String>,
}

#[test]
fn member_model_settings_are_project_scoped_and_dispatched_round_keeps_its_selection() {
    use crate::multi_agent::conversation_models::Selection;
    use serde_json::json;
    let fixture = Fixture::new();
    let store = &fixture.store;
    let member = &fixture.agents[0];
    let kind = AgentRuntimeKind::Codex;
    let catalog = [json!({"id":"model","type":"select","currentValue":"model-a","options":[
        {"group":"Provider","options":[{"value":"model-a","name":"A"},{"value":"model-b","name":"B"}]}]}),
        json!({"id":"effort","type":"select","options":[{"value":"high","name":"High"}]}),
        json!({"id":"mode","type":"select","options":[{"value":"full-access","name":"Full"}]})];
    let loaded = store.load_member_models_with(&fixture.project, member, false, |runtime, saved| {
        assert_eq!(runtime, kind);
        assert_eq!(saved, None);
        Ok(catalog.to_vec())
    }).unwrap();
    assert_eq!(serde_json::to_value(loaded).unwrap()["model"][1]["value"], "model-b");
    assert!(store.conversation_snapshot(&fixture.project).unwrap().rounds.is_empty());
    store.load_member_models_with(&fixture.project, member, false, |_, _| panic!("cached catalog should not start an adapter")).unwrap();
    let selected = Selection {model:Some("model-b".into()), reasoning:Some("high".into())};
    assert!(store.set_conversation_member_models("unrelated-project", member, selected.clone()).is_err());
    assert!(store.set_conversation_member_models(&fixture.project, member, Selection {
        model:None, reasoning:Some("full-access".into()),
    }).is_err());
    store.set_conversation_member_models(&fixture.project, member, selected).unwrap();
    let mut input = fixture.input();
    input.native_capabilities = true;
    let (round, _) = store.begin_conversation(input).unwrap();
    store.set_conversation_member_models(&fixture.project, member, Selection::default()).unwrap();
    let turn = prepare_turn(store, &round.id, 1, "chat", 0, member).unwrap();
    let selection = store.discussion_model_selection(&turn.task_id, member, kind).unwrap();
    assert_eq!(selection.model.as_deref(), Some("model-b"));
    assert_eq!(selection.reasoning.as_deref(), Some("high"));
    assert!(!store.discussion_model_selection(&turn.task_id, &fixture.agents[1], kind).unwrap().is_custom());
    store.finish_conversation(&round.id, "completed", None).unwrap();
    let mut input = fixture.input();
    input.native_capabilities = true;
    input.client_message_id = "user-2".into();
    let (next_round, _) = store.begin_conversation(input).unwrap();
    let next = prepare_turn(store, &next_round.id, 1, "chat", 0, member).unwrap();
    assert!(!store.discussion_model_selection(&next.task_id, member, kind).unwrap().is_custom());
}

#[test]
fn conversation_attachments_are_snapshotted_scoped_and_delivered() {
    let fixture = Fixture::new();
    let conversation = fixture.store.conversation_snapshot(&fixture.project).unwrap().conversation.id;
    let document = fixture.root.join("需求.md");
    let image = fixture.root.join("截图.png");
    let video = fixture.root.join("操作演示.MP4");
    std::fs::write(&document, "ORIGINAL_ATTACHMENT_TEXT").unwrap();
    let png = b"\x89PNG\r\n\x1a\n";
    std::fs::write(&image, png).unwrap();
    let mut clip = b"\x00\x00\x00\x18ftypisom\x00\x00\x02\x00isommp42".to_vec();
    clip.extend_from_slice(b"\x00\x00\x20\x08mdat");
    clip.extend(std::iter::repeat_n(0x14, 8192));
    std::fs::write(&video, &clip).unwrap();
    let draft = |path: &std::path::Path| ConversationAttachmentSource::Draft { path: path.to_path_buf() };
    assert_eq!(fixture.store.conversation_image_bytes(draft(&image)).unwrap(), png);
    assert!(fixture.store.conversation_image_bytes(draft(&document)).is_err());
    assert!(fixture.store.conversation_image_bytes(draft(std::path::Path::new("截图.png"))).is_err());
    assert_eq!(fixture.store.conversation_video_path(draft(&video)).unwrap(), video.canonicalize().unwrap());
    assert!(fixture.store.conversation_video_path(draft(&image)).is_err());
    assert!(fixture.store.conversation_video_path(draft(std::path::Path::new("操作演示.MP4"))).is_err());
    let files = fixture.store.stage_conversation_attachments(&fixture.project, &conversation,
        &[document.display().to_string(), image.display().to_string(), video.display().to_string()]).unwrap();
    assert_eq!(files[2].media_type, "video/mp4");
    assert_eq!(files[2].bytes, clip.len() as u64);
    std::fs::write(&document, "CHANGED_SOURCE").unwrap();
    std::fs::remove_file(&image).unwrap();
    std::fs::remove_file(&video).unwrap();
    let mut input = fixture.input();
    input.body.clear();
    input.expected_conversation_id = Some(conversation.clone());
    input.attachment_ids = files.iter().map(|file| file.id.clone()).collect();
    let (round, started) = fixture.store.begin_conversation(input.clone()).unwrap();
    assert!(started);
    assert!(!fixture.store.begin_conversation(input.clone()).unwrap().1);
    input.attachment_ids.reverse();
    assert!(fixture.store.begin_conversation(input).is_err());
    let turn = prepare_turn(&fixture.store, &round.id, 1, "chat", 0, &fixture.agents[0]).unwrap();
    let (_, snapshot) = fixture.store.export_conversation_markdown(&fixture.project, &conversation).unwrap();
    assert!(snapshot.contains("附件：需求.md"));
    let images = fixture.store.conversation_task_images(&turn.task_id).unwrap();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].1, "image/png");
    assert_eq!(std::fs::read(&images[0].0).unwrap(), png);
    let prompt = discussion_prompt(&fixture.store, &fixture.store.connection.lock().unwrap(),
        &round, "chat", 0, &fixture.agents[0], None, None).unwrap();
    assert!(prompt.contains("ORIGINAL_ATTACHMENT_TEXT"));
    assert!(!prompt.contains("CHANGED_SOURCE"));
    assert!(prompt.contains("操作演示.MP4") && prompt.contains("video/mp4"));
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    let snapshot = reopened.conversation_snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.messages[0].attachments, files);
    let sent = |conversation_id: String, attachment_id: String| ConversationAttachmentSource::Message {
        project_id: fixture.project.clone(), conversation_id, message_id: round.user_message_id.clone(), attachment_id,
    };
    assert_eq!(reopened.conversation_image_bytes(sent(conversation.clone(), files[1].id.clone())).unwrap(), png);
    assert!(reopened.conversation_image_bytes(sent(id(), files[1].id.clone())).is_err());
    assert!(reopened.conversation_image_bytes(sent(conversation.clone(), files[0].id.clone())).is_err());
    assert!(reopened.conversation_image_bytes(draft(&image)).is_err());
    assert!(reopened.conversation_attachment_path(&fixture.project, &id(), &round.user_message_id, &files[0].id).is_err());
    let copied = reopened.conversation_attachment_path(&fixture.project, &conversation, &round.user_message_id, &files[0].id).unwrap();
    assert_eq!(std::fs::read_to_string(copied).unwrap(), "ORIGINAL_ATTACHMENT_TEXT");
    let copied_video = reopened.conversation_attachment_path(&fixture.project, &conversation, &round.user_message_id, &files[2].id).unwrap();
    assert_eq!(reopened.conversation_video_path(sent(conversation.clone(), files[2].id.clone())).unwrap(), copied_video.canonicalize().unwrap());
    assert!(reopened.conversation_video_path(sent(id(), files[2].id.clone())).is_err());
    assert!(reopened.conversation_video_path(sent(conversation.clone(), id())).is_err());
    assert_eq!(std::fs::read(copied_video).unwrap(), clip);
    assert!(reopened.conversation_image_bytes(sent(conversation, files[2].id.clone())).is_err());
}

#[test]
fn invalid_conversation_attachment_never_queues_a_turn() {
    let fixture = Fixture::new();
    let conversation = fixture.store.conversation_snapshot(&fixture.project).unwrap().conversation.id;
    let wrong_type = fixture.root.join("假图片.png");
    std::fs::write(&wrong_type, "not an image").unwrap();
    assert!(fixture.store.conversation_image_bytes(ConversationAttachmentSource::Draft { path: wrong_type.clone() }).is_err());
    assert!(fixture.store.stage_conversation_attachments(&fixture.project, &conversation, &[wrong_type.display().to_string()]).is_err());
    let wrong_video = fixture.root.join("假视频.mp4");
    std::fs::write(&wrong_video, b"not an mp4 video").unwrap();
    let oversized = fixture.root.join("超限.webm");
    std::fs::File::create(&oversized).unwrap().set_len(100 * 1024 * 1024 + 1).unwrap();
    let stage = |path: &std::path::Path| fixture.store.stage_conversation_attachments(&fixture.project, &conversation, &[path.display().to_string()]);
    assert!(stage(&wrong_video).unwrap_err().to_string().contains("附件内容与文件格式不符"));
    assert!(stage(&oversized).unwrap_err().to_string().contains("超过 100 MB"));
    assert!(fixture.store.conversation_video_path(ConversationAttachmentSource::Draft { path: wrong_video }).is_err());
    assert!(fixture.store.conversation_video_path(ConversationAttachmentSource::Draft { path: oversized }).is_err());
    let staged = attachments::scope(&fixture.store.conversation_attachment_root, &fixture.project, &conversation).unwrap();
    assert!(std::fs::read_dir(staged).unwrap().next().is_none());
    let mut input = fixture.input();
    input.attachment_ids = vec![id()];
    assert!(fixture.store.interrupt_conversation(interjection(&input)).is_err());
    assert!(fixture.store.begin_conversation(input).is_err());
    assert!(fixture.store.conversation_snapshot(&fixture.project).unwrap().messages.is_empty());
}

#[test]
fn assigned_team_chat_keeps_all_recipients_and_launches_only_the_owner() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.body = "谁能帮我启动这个项目？".into();
    input.chat_dispatch = Some(ConversationChatDispatch::Assigned { agent_id: id() });
    assert!(fixture.store.begin_conversation(input.clone()).is_err());
    input.chat_dispatch = Some(ConversationChatDispatch::Assigned { agent_id: fixture.agents[1].clone() });
    let executor = ScriptedReplies::new(vec!["服务已启动".into()]);
    let chat = fixture.run_input(input.clone(), &executor);
    assert_eq!(chat.rounds[0].participant_agent_ids, fixture.agents);
    assert_eq!(chat.rounds[0].chat_dispatch, input.chat_dispatch);
    assert_eq!(chat.rounds[0].status, "completed");
    let calls = executor.calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, fixture.agents[1]);
}

struct ParallelReplies {
    active: std::sync::atomic::AtomicUsize,
    started: std::sync::atomic::AtomicUsize,
    peak: std::sync::atomic::AtomicUsize,
}
impl DiscussionExecutor for ParallelReplies {
    fn execute(&self, store: &MultiAgentStore, _task: &str, run: &str, agent: &str, _: &AtomicBool) -> Result<()> {
        store.update_run(UpdateRunInput { run_id: run.into(), status: RunStatus::Running,
            output_summary: None, error: None, session_locator: None })?;
        let active = self.active.fetch_add(1, Ordering::AcqRel) + 1;
        self.peak.fetch_max(active, Ordering::AcqRel);
        self.started.fetch_add(1, Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(5);
        while self.started.load(Ordering::Acquire) < 3 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let result = store.finalize_successful_run(run, "isolated", &format!("成员 {agent} 的回复"), false, None, None);
        self.active.fetch_sub(1, Ordering::AcqRel);
        result
    }
}

#[test]
fn team_chat_runs_independent_replies_with_bounded_parallelism() {
    let mut fixture = Fixture::new();
    for index in 2..5 {
        let member = fixture.store.create_agent(CreateAgentInput {
            avatar_id: None, project_id: fixture.project.clone(), name: format!("member-{index}"),
            runtime_kind: AgentRuntimeKind::Codex, role: AgentRole::Worker, description: String::new(),
            persona: Default::default(), persona_custom: String::new(), model: None,
            permission_profile: PermissionProfile::Default, max_concurrency: 1,
        }).unwrap();
        fixture.agents.push(member.id);
    }
    let mut input = fixture.input();
    input.body = "大家各自介绍一下".into();
    input.chat_dispatch = Some(ConversationChatDispatch::Parallel);
    let replies = ParallelReplies { active: 0.into(), started: 0.into(), peak: 0.into() };
    let chat = fixture.run_input(input, &replies);
    assert_eq!(replies.peak.load(Ordering::Acquire), 3, "three agents must overlap");
    assert_eq!(replies.started.load(Ordering::Acquire), 5);
    assert_eq!(chat.rounds[0].status, "completed", "{:?}", chat.rounds[0].stop_reason);
    let replies = chat.messages.iter().filter(|message| message.sender_type == "agent").collect::<Vec<_>>();
    assert_eq!(replies.len(), 5);
    assert!(replies.iter().all(|reply| reply.status == "completed"
        && reply.reply_to_message_id.as_deref() == Some(chat.rounds[0].user_message_id.as_str())));
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
        ConversationSendInput { native_capabilities: false, managed: false, integration_owner: None, expected_conversation_id: None, chat_dispatch: None,
            project_id: self.project.clone(),
            client_message_id: "user-1".into(),
            body: "Discuss the project".into(),
            attachment_ids: Vec::new(),
            participant_agent_ids: self.agents.clone(),
            mode: "discussion".into(),
            intent: "chat".into(),
            max_rounds: 1,
            max_duration_seconds: 60,
            task_rules: None,
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
            final_task_key: None,
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
    ConversationInterruptInput { native_capabilities: None, expected_conversation_id: input.expected_conversation_id.clone(), project_id: input.project_id.clone(), client_message_id: input.client_message_id.clone(),
        body: input.body.clone(), attachment_ids: input.attachment_ids.clone(), participant_agent_ids: Some(input.participant_agent_ids.clone()), mode: Some(input.mode.clone()),
        intent: Some(input.intent.clone()), chat_dispatch: input.chat_dispatch.clone(), max_rounds: Some(input.max_rounds), max_duration_seconds: Some(input.max_duration_seconds), task_rules: input.task_rules.clone() }
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
fn plan_preflight_reports_missing_git_commit_reviewers_and_integrator() {
    let fixture = Fixture::new();
    fixture.store.connection.lock().unwrap().execute(
        "UPDATE ma_agents SET runtime_kind='claude_code' WHERE id=?1", [&fixture.agents[1]],
    ).unwrap();
    let mut input = fixture.input();
    input.body = "开发一个贪吃蛇小游戏。前后端分别开发，交叉审查后再由集成成员联调。".into();
    assert!(fixture.store.conversation_plan_preflight(&input).unwrap().is_empty());
    input.mode = "execution_plan".into();
    let issues = fixture.store.conversation_plan_preflight(&input).unwrap();
    assert_eq!(issues.len(), 4);
    assert!(issues.iter().any(|issue| issue.kind == "git" && issue.message.contains("缺少 Git 工作区")));
    assert!(issues.iter().any(|issue| issue.message.contains("Runtime 为 Claude Code、角色为「审查」")));
    assert!(issues.iter().any(|issue| issue.message.contains("Runtime 为 Codex、角色为「审查」")));
    assert!(issues.iter().any(|issue| issue.message.contains("缺少「集成」角色")));
    assert!(fixture.store.conversation_snapshot(&fixture.project).unwrap().messages.is_empty());

    let status = std::process::Command::new("git").arg("-C").arg(fixture.root.join("project"))
        .args(["init", "-b", "main"]).output().unwrap().status;
    assert!(status.success());
    let issues = fixture.store.conversation_plan_preflight(&input).unwrap();
    assert!(issues.iter().any(|issue| issue.kind == "git" && issue.message.contains("首次提交")));

    fixture.git();
    for (name, runtime_kind, role) in [
        ("审查 Codex", AgentRuntimeKind::ClaudeCode, AgentRole::Reviewer),
        ("审查 Claude", AgentRuntimeKind::Codex, AgentRole::Reviewer),
        ("联调", AgentRuntimeKind::Codex, AgentRole::Integrator),
    ] {
        let agent = fixture.store.create_agent(CreateAgentInput {
            avatar_id: None, project_id: fixture.project.clone(), name: name.into(), runtime_kind, role,
            description: String::new(), persona: Default::default(), persona_custom: String::new(), model: None,
            permission_profile: PermissionProfile::Default, max_concurrency: 1,
        }).unwrap();
        input.participant_agent_ids.push(agent.id);
    }
    assert!(fixture.store.conversation_plan_preflight(&input).unwrap().is_empty());
    input.participant_agent_ids.truncate(2);
    input.body = "开发一个贪吃蛇小游戏，交给其中一位开发成员。".into();
    assert!(fixture.store.conversation_plan_preflight(&input).unwrap().is_empty());
    input.body = "开发一个贪吃蛇小游戏，交给其中一位开发成员。完成后安排只读审查。".into();
    let issues = fixture.store.conversation_plan_preflight(&input).unwrap();
    assert_eq!(issues.len(), 1);
    assert!(issues[0].message.contains("缺少能执行只读审查的成员"));
}

#[test]
fn conversation_export_preserves_complete_markdown_and_current_session_scope() {
    let fixture = Fixture::new();
    let markdown = "导出测试\n\n```ts\nconst name = '中文';\n```";
    let mut input = fixture.input();
    input.body = markdown.into();
    let first = fixture.run_input(input, &Replies::default());
    let conversation_id = &first.conversation.id;
    {
        let connection = fixture.store.connection.lock().unwrap();
        connection.execute("UPDATE ma_projects SET name=?1 WHERE id=?2", params!["项目/A?", &fixture.project]).unwrap();
        for number in 0..101 {
            let mut message = first.messages[0].clone();
            message.id = id();
            message.client_message_id = None;
            message.created_at -= 102 - number;
            message.body = format!("较早消息 {number:03}");
            save_message(&connection, &format!("export-fixture:{number}"), &message).unwrap();
        }
    }
    let (filename, exported) = fixture.store.export_conversation_markdown(&fixture.project, conversation_id).unwrap();
    assert!(filename.starts_with("团队会话-项目_A_-") && filename.ends_with(".md"));
    assert!(exported.starts_with("# 项目/A? · 团队会话"));
    assert!(exported.contains(markdown) && exported.contains("## member-0 · "));
    assert!(exported.find("较早消息 000").unwrap() < exported.find(markdown).unwrap());
    assert_eq!(exported.matches("较早消息 ").count(), 101);
    assert!(fixture.store.export_conversation_markdown("wrong-project", conversation_id).is_err());

    let complete = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    let fresh = fixture.store.new_conversation(clear_input(&complete)).unwrap();
    assert!(fixture.store.export_conversation_markdown(&fixture.project, conversation_id).unwrap_err().to_string().contains("已变化"));
    assert!(fixture.store.export_conversation_markdown(&fixture.project, &fresh.conversation.id).unwrap_err().to_string().contains("暂无"));
    fixture.store.switch_conversation(ConversationSwitchInput {
        project_id: fixture.project.clone(),
        expected_conversation_id: fresh.conversation.id,
        conversation_id: conversation_id.clone(),
    }).unwrap();
    assert_eq!(fixture.store.export_conversation_markdown(&fixture.project, conversation_id).unwrap().1, exported);
}

#[test]
fn work_queue_restores_saved_requests_without_crossing_project_or_cleared_history() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    let old = fixture.run(&Replies::default());
    let draft = fixture.draft();
    let plan = store.save_conversation_plan(&old.rounds[0].id, &old.messages.last().unwrap().id,
        &json!({"summary":draft.summary,"tasks":draft.tasks}).to_string()).unwrap();
    let saved = store.conversation_snapshot(&fixture.project).unwrap();
    let next = store.new_conversation(clear_input(&saved)).unwrap();
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    let snapshot = reopened.conversation_snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.conversation.id, next.conversation.id);
    let item = snapshot.work_items.iter().find(|item| item.kind == "plan" && item.id == plan.id).unwrap();
    assert_eq!(item.conversation_id, saved.conversation.id);
    assert_eq!(item.message_id, plan.message_id);
    assert_eq!(item.request, fixture.input().body);
    let other = reopened.create_project(CreateProjectInput {
        name: "Other".into(), goal: "Separate project".into(),
        root_path: fixture.root.join("other").display().to_string(), default_branch: "main".into(),
    }).unwrap();
    assert!(reopened.conversation_snapshot(&other.id).unwrap().work_items.is_empty());
    let original = reopened.switch_conversation(ConversationSwitchInput {
        project_id: fixture.project.clone(), expected_conversation_id: next.conversation.id, conversation_id: item.conversation_id.clone(),
    }).unwrap();
    assert!(original.messages.iter().any(|message| message.id == item.message_id));
    assert!(original.execution_plans.iter().any(|record| record.id == item.id && record.status == "pending"));
    let cleared = reopened.clear_conversation(clear_input(&original)).unwrap();
    assert!(cleared.work_items.iter().all(|record| record.conversation_id != original.conversation.id));
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
fn conversation_history_restores_each_context_and_keeps_cleared_records_hidden() {
    let fixture = Fixture::new();
    let old = fixture.run(&Replies::default());
    let fresh = fixture.store.new_conversation(clear_input(&old)).unwrap();
    let switch = |from: &ConversationSnapshot, to: &ConversationSnapshot| ConversationSwitchInput {
        project_id: fixture.project.clone(),
        expected_conversation_id: from.conversation.id.clone(),
        conversation_id: to.conversation.id.clone(),
    };
    let mut input = fixture.input();
    input.client_message_id = "new-session-request".into();
    input.body = "Only in the new session".into();
    input.expected_conversation_id = Some(old.conversation.id.clone());
    assert!(fixture.store.begin_conversation(input.clone()).unwrap_err().to_string().contains("已变化"));
    assert!(fixture.store.interrupt_conversation(interjection(&input)).unwrap_err().to_string().contains("已变化"));
    input.expected_conversation_id = Some(fresh.conversation.id.clone());
    let latest = fixture.run_input(input, &Replies::default());

    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    let entries = reopened.conversation_history(&fixture.project).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|item| item.id == old.conversation.id && !item.current && !item.legacy
        && item.preview.as_deref() == Some("Discuss the project")));
    assert!(entries.iter().any(|item| item.id == latest.conversation.id && item.current));
    assert!(reopened.switch_conversation(switch(&old, &old)).is_err());
    let restored = reopened.switch_conversation(switch(&latest, &old)).unwrap();
    assert_eq!(restored.conversation.created_at, old.conversation.created_at);
    assert_eq!(restored.messages.iter().map(|message| &message.id).collect::<Vec<_>>(),
        old.messages.iter().map(|message| &message.id).collect::<Vec<_>>());
    let mut resumed_input = fixture.input();
    resumed_input.client_message_id = "resumed-session-request".into();
    resumed_input.body = "Continue the old discussion".into();
    resumed_input.expected_conversation_id = Some(old.conversation.id.clone());
    let executor = Replies::default();
    let continued = fixture.run_input(resumed_input, &executor);
    let prompts = executor.prompts.lock().unwrap();
    assert!(prompts[0].contains("Discuss the project"));
    assert!(!prompts[0].contains("Only in the new session"));
    assert_eq!(reopened.switch_conversation(switch(&old, &latest)).unwrap().messages[0].body, "Only in the new session");
    let empty = reopened.clear_conversation(clear_input(&latest)).unwrap();
    assert!(reopened.switch_conversation(switch(&empty, &latest)).is_err());
    assert!(reopened.conversation_history(&fixture.project).unwrap().iter().all(|entry| entry.id != latest.conversation.id));
    assert!(reopened.conversation_history("another-project").is_err());

    // Simulate a pre-history database where the rotated messages had no catalog entry.
    reopened.connection.lock().unwrap().execute("DELETE FROM ma_conversation_history WHERE id=?1", [&old.conversation.id]).unwrap();
    let migrated = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    let history = migrated.conversation_history(&fixture.project).unwrap();
    assert!(history.iter().any(|entry| entry.id == old.conversation.id && entry.legacy));
    assert!(history.iter().all(|entry| entry.id != latest.conversation.id));
    assert_eq!(migrated.switch_conversation(switch(&empty, &old)).unwrap().messages.len(), continued.messages.len());
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
fn project_rename_preserves_conversation_and_execution_records() -> Result<()> {
    let fixture = Fixture::new();
    let store = &fixture.store;
    fixture.run(&Replies::default());
    let chat = serde_json::to_value(store.conversation_snapshot(&fixture.project)?)?;
    let mut execution = serde_json::to_value(store.snapshot(&fixture.project)?)?;
    assert!(!execution["tasks"].as_array().unwrap().is_empty());
    for name in ["  ", "间隔\n换行"] {
        assert!(store.rename_project(&fixture.project, name).is_err());
    }
    assert!(store.rename_project("missing-project", "新名称").is_err());
    assert_eq!(serde_json::to_value(store.snapshot(&fixture.project)?)?, execution);

    let renamed = store.rename_project(&fixture.project, " 新名称 ")?;
    assert_eq!(renamed.id, fixture.project);
    assert_eq!(renamed.name, "新名称");
    execution["project"]["name"] = json!("新名称");
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db"))?;
    assert_eq!(serde_json::to_value(reopened.snapshot(&fixture.project)?)?, execution);
    assert_eq!(serde_json::to_value(reopened.conversation_snapshot(&fixture.project)?)?, chat);
    Ok(())
}

#[test]
fn conversation_sessions_rename_history_and_delete_current_durably() {
    let fixture = Fixture::new();
    let previous = fixture.run(&Replies::default());
    let current = fixture.store.new_conversation(clear_input(&previous)).unwrap();
    let rename = |id: &str, title: &str| ConversationRenameInput {
        project_id: fixture.project.clone(), conversation_id: id.into(), title: title.into(),
    };
    assert!(fixture.store.rename_conversation(rename(&previous.conversation.id, " ")).is_err());
    let renamed = fixture.store.rename_conversation(rename(&previous.conversation.id, " 界面讨论 ")).unwrap();
    assert_eq!(renamed.conversation.id, current.conversation.id);
    let renamed = fixture.store.rename_conversation(rename(&current.conversation.id, "发布准备")).unwrap();
    assert_eq!(renamed.conversation.title.as_deref(), Some("发布准备"));
    assert!(fixture.store.delete_conversation(clear_input(&previous)).is_err());
    let next = fixture.store.delete_conversation(clear_input(&current)).unwrap();
    assert_ne!(next.conversation.id, current.conversation.id);
    assert!(next.conversation.title.is_none() && next.messages.is_empty());

    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    let entries = reopened.conversation_history(&fixture.project).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.iter().any(|entry| entry.id == previous.conversation.id && entry.title.as_deref() == Some("界面讨论")));
    assert!(entries.iter().all(|entry| entry.id != current.conversation.id));
    let switch = |id: &str| ConversationSwitchInput {
        project_id: fixture.project.clone(), expected_conversation_id: next.conversation.id.clone(), conversation_id: id.into(),
    };
    assert!(reopened.switch_conversation(switch(&current.conversation.id)).is_err());
    assert!(reopened.rename_conversation(rename(&current.conversation.id, "恢复")).is_err());
    let restored = reopened.switch_conversation(switch(&previous.conversation.id)).unwrap();
    assert_eq!(restored.conversation.title.as_deref(), Some("界面讨论"));
    assert_eq!(restored.messages.len(), previous.messages.len());
}

#[test]
fn conversation_sessions_delete_protects_pending_work_and_preserves_applied_plan() {
    let fixture = Fixture::new();
    fixture.git();
    let initial = fixture.run(&Replies::default());
    let draft = fixture.draft();
    let plan = fixture.store.save_conversation_plan(&initial.rounds[0].id,
        &initial.messages.last().unwrap().id, &json!({"summary":draft.summary,"tasks":draft.tasks}).to_string()).unwrap();
    let current = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert!(fixture.store.delete_conversation(clear_input(&current)).unwrap_err().to_string().contains("待处理"));

    let tasks = fixture.store.apply_conversation_plan(&ConversationExecutePlanInput {
        project_id: fixture.project.clone(), plan_id: plan.id.clone(), client_request_id: "delete-session-test".into(),
    }).unwrap();
    let current = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert!(fixture.store.delete_conversation(clear_input(&current)).unwrap_err().to_string().contains("待处理"));

    // Fully finished engineering evidence remains reachable after the transcript is removed.
    let connection = fixture.store.connection.lock().unwrap();
    for task in &tasks {
        connection.execute("UPDATE ma_tasks SET status='done' WHERE id=?1", [task]).unwrap();
    }
    drop(connection);
    let before = fixture.store.snapshot(&fixture.project).unwrap();
    let next = fixture.store.delete_conversation(clear_input(&current)).unwrap();
    assert!(next.messages.is_empty() && next.execution_plans.is_empty());
    assert!(next.retained_execution_plans.iter().any(|retained| retained.id == plan.id && retained.task_ids == tasks));
    assert_eq!(serde_json::to_value(before).unwrap(), serde_json::to_value(fixture.store.snapshot(&fixture.project).unwrap()).unwrap());
    assert!(fixture.store.conversation_history(&fixture.project).unwrap().iter().all(|entry| entry.id != current.conversation.id));
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
    fixture.store.interrupt_conversation(ConversationInterruptInput { native_capabilities: None,project_id: other.id.clone(),client_message_id:"other-message".into(),body:"KEEP_OTHER_PROJECT".into(),..Default::default()}).unwrap();
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
    fixture.store.interrupt_conversation(ConversationInterruptInput { native_capabilities: None,project_id: fixture.project.clone(),client_message_id:"arrived-after-confirm".into(),body:"NEW_MESSAGE_AFTER_CONFIRM".into(),..Default::default()}).unwrap();
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
fn background_chat_host_requires_an_ordinary_chat_round() {
    for (mode, intent, expected) in [
        ("discussion", "chat", true),
        ("discussion", "exchange", false),
        ("execution_plan", "chat", false),
    ] {
        let fixture = Fixture::new();
        let input = ConversationSendInput { mode: mode.into(), intent: intent.into(), ..fixture.input() };
        let (round, _) = fixture.store.begin_conversation(input).unwrap();
        let turn = prepare_turn(&fixture.store, &round.id, 0, &round.phase, 0, &fixture.agents[0]).unwrap();
        assert_eq!(fixture.store.is_plain_chat_turn(&turn.task_id).unwrap(), expected, "{mode} {intent}");
        assert!(!fixture.store.is_plain_chat_turn("unrelated-task").unwrap());
    }
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
            store.interrupt_conversation(ConversationInterruptInput { native_capabilities: None,
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
fn stopping_parallel_chat_settles_all_running_replies() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.chat_dispatch = Some(ConversationChatDispatch::Parallel);
    let (round, _) = fixture.store.begin_conversation(input).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    std::thread::scope(|scope| {
        let supervisor = scope.spawn(|| run_discussion(&fixture.store, &round.id, &cancelled,
            &StopDuringRun { unconfirmed: false, interrupt: false }));
        let deadline = Instant::now() + Duration::from_secs(2);
        let overlapping = loop {
            let running = fixture.store.snapshot(&fixture.project).unwrap().runs.iter()
                .filter(|run| run.status == RunStatus::Running).count();
            if running == 2 || Instant::now() >= deadline { break running; }
            std::thread::sleep(Duration::from_millis(5));
        };
        fixture.store.request_conversation_stop(&fixture.project, "paused", "测试暂停", None).unwrap();
        supervisor.join().unwrap().unwrap();
        assert_eq!(overlapping, 2);
    });
    assert_eq!(fixture.store.conversation_round(&round.id).unwrap().status, "paused");
    let runs = fixture.store.snapshot(&fixture.project).unwrap().runs;
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|run| run.status == RunStatus::Cancelled));
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
fn delivery_receipts_follow_the_exact_member_run_and_survive_reopening() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    let (round, _) = store.begin_conversation(fixture.input()).unwrap();
    let turn = prepare_turn(store, &round.id, 0, "chat", 0, &fixture.agents[0]).unwrap();
    let run = store.claim_task(ClaimTaskInput {
        task_id: turn.task_id, agent_id: fixture.agents[0].clone(), lease_seconds: 120,
    }).unwrap();
    store.update_run(UpdateRunInput { run_id: run.id.clone(), status: RunStatus::Running,
        output_summary: None, error: None, session_locator: None }).unwrap();
    let receipts = |store: &MultiAgentStore| store.conversation_snapshot(&fixture.project).unwrap()
        .delivery_receipts.remove(&round.user_message_id).unwrap();
    let initial = receipts(store);
    assert_eq!(initial[0].status, "awaiting_receipt");
    assert_eq!(initial[0].run_id.as_deref(), Some(run.id.as_str()));
    assert_eq!(initial[1].status, "queued");
    store.append_runtime_event(super::super::AppendRuntimeEventInput {
        run_id: run.id.clone(), event_type: "conversation_prompt_received".into(), payload: json!({}),
    }).unwrap();
    assert_eq!(receipts(store)[0].status, "received");
    store.append_runtime_event(super::super::AppendRuntimeEventInput {
        run_id: run.id.clone(), event_type: "analysis_activity".into(), payload: json!({}),
    }).unwrap();
    assert_eq!(receipts(store)[0].status, "running");
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    reopened.recover_interrupted_runs_at_startup().unwrap();
    reopened.recover_conversations_at_startup().unwrap();
    assert_eq!(receipts(&reopened)[0].status, "unconfirmed");
    assert!(!reopened.begin_conversation(fixture.input()).unwrap().1);
}

#[test]
fn topic_resume_is_member_and_topic_scoped_and_current_request_appears_once() {
    let fixture = Fixture::new();
    let store = &fixture.store;
    let agent = &fixture.agents[0];
    let binding = json!({"runtime":"codex","permissionProfile":"default","version":1});
    let (round, _) = store.begin_conversation(fixture.input()).unwrap();
    let turn = prepare_turn(store, &round.id, 0, "chat", 0, agent).unwrap();
    let run = store.claim_task(ClaimTaskInput {
        task_id: turn.task_id, agent_id: agent.clone(), lease_seconds: 120,
    }).unwrap();
    store.update_run(UpdateRunInput { run_id: run.id.clone(), status: RunStatus::Running,
        output_summary: None, error: None, session_locator: None }).unwrap();
    store.update_run(UpdateRunInput { run_id: run.id, status: RunStatus::Succeeded,
        output_summary: Some("old topic response".into()), error: None,
        session_locator: Some(json!({"topicBinding":binding,"sessionId":"provider-topic-a"})) }).unwrap();
    store.finish_conversation(&round.id, "completed", None).unwrap();
    let mut request = fixture.input();
    request.client_message_id = "topic-a-2".into();
    request.body = "unique-current-request-marker".into();
    let (next, _) = store.begin_conversation(request).unwrap();
    let next_turn = prepare_turn(store, &next.id, 0, "chat", 0, agent).unwrap();
    let (locator, cutoff, prior) = store.topic_session(&next_turn.task_id, &fixture.project, agent, &binding).unwrap().unwrap();
    assert_eq!(locator["sessionId"], "provider-topic-a");
    assert!(store.topic_session(&next_turn.task_id, &fixture.project, &fixture.agents[1], &binding).unwrap().is_none());
    assert!(store.topic_session(&next_turn.task_id, &fixture.project, agent, &json!({"version":2})).unwrap().is_none());
    let prompt = store.topic_delta_prompt(&next_turn.task_id, cutoff, &prior).unwrap();
    assert_eq!(prompt.matches("unique-current-request-marker").count(), 1);
    assert!(!prompt.contains("old topic response"));
    store.finish_conversation(&next.id, "completed", None).unwrap();
    let snapshot = store.conversation_snapshot(&fixture.project).unwrap();
    let topic = store.new_conversation(ConversationClearInput {
        project_id: fixture.project.clone(), expected_conversation_id: snapshot.conversation.id,
        expected_message_ids: snapshot.messages.into_iter().map(|message| message.id).collect(),
    }).unwrap();
    let mut request = fixture.input();
    request.client_message_id = "topic-b-1".into();
    request.expected_conversation_id = Some(topic.conversation.id);
    let (other, _) = store.begin_conversation(request).unwrap();
    let other_turn = prepare_turn(store, &other.id, 0, "chat", 0, agent).unwrap();
    assert!(store.topic_session(&other_turn.task_id, &fixture.project, agent, &binding).unwrap().is_none());
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
        ".git/",
        "frontend//",
        "/frontend/",
        "/",
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
    overlap.tasks[0].write_paths = vec!["src/owned/".into()];
    let mut second = overlap.tasks[0].clone();
    second.key = "two".into();
    second.assignee_agent_id = fixture.agents[1].clone();
    second.write_paths = vec!["src\\owned\\nested".into()];
    overlap.tasks.push(second);
    let error = validate_plan(&connection, &round, &overlap).unwrap_err();
    assert!(error.to_string().contains("写入范围重叠"), "{error}");
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
    let make_task = |title| fixture.store.create_task(serde_json::from_value(json!({
        "projectId": fixture.project, "title": title, "assigneeAgentId": fixture.agents[0], "maxAttempts": 1,
    })).unwrap()).unwrap();
    let failed = make_task("旧任务");
    let dependent = make_task("依赖旧任务");
    fixture.store.add_dependency(crate::multi_agent::AddDependencyInput {
        task_id: dependent.id.clone(), depends_on_task_id: failed.id.clone(),
    }).unwrap();
    assert!(fixture.store.apply_conversation_plan(&input).unwrap_err().to_string().contains("2 项"));
    let old_run = fixture.store.claim_task(ClaimTaskInput {
        task_id: failed.id.clone(), agent_id: fixture.agents[0].clone(), lease_seconds: 120,
    }).unwrap();
    assert!(fixture.store.apply_conversation_plan(&input).is_err());
    fixture.store.fail_run_permanently(&old_run.id, "old failure", Some(json!({"stopUnverified":true}))).unwrap();
    assert!(fixture.store.apply_conversation_plan(&input).unwrap_err().to_string().contains("未确认停止"));
    fixture.store.connection.lock().unwrap().execute(
        "UPDATE ma_runs SET session_locator_json=NULL WHERE id=?1", [&old_run.id],
    ).unwrap();
    let tasks = fixture.store.apply_conversation_plan(&input).unwrap();
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        fixture.store.apply_conversation_plan(&input).unwrap(),
        tasks
    );
    let snapshot = fixture.store.snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.tasks.iter().find(|t| t.id == failed.id).unwrap().status, crate::multi_agent::TaskStatus::Failed);
    assert_eq!(snapshot.tasks.iter().find(|t| t.id == dependent.id).unwrap().status, crate::multi_agent::TaskStatus::Blocked);
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
fn design_delegation_blocks_only_its_parent_and_resumes_after_a_scoped_delivery() {
    use crate::multi_agent::TaskStatus;
    let fixture = Fixture::new();
    fixture.git();
    let parallel = fixture.store.create_agent(CreateAgentInput {
        avatar_id: None, project_id: fixture.project.clone(), name: "前端 Agent".into(),
        runtime_kind: AgentRuntimeKind::Codex, role: AgentRole::Worker, description: "前端开发".into(),
        persona: Default::default(), persona_custom: String::new(), model: None,
        permission_profile: PermissionProfile::Default, max_concurrency: 1,
    }).unwrap();
    let mut chat = fixture.input();
    chat.participant_agent_ids.push(parallel.id.clone());
    let snapshot = fixture.run_input(chat, &Replies::default());
    let conversation = snapshot.conversation.id;
    let mut draft = fixture.draft();
    let mut frontend = draft.tasks[0].clone();
    frontend.key = "frontend".into();
    frontend.title = "实现前端".into();
    frontend.assignee_agent_id = parallel.id.clone();
    frontend.write_paths = vec!["web".into()];
    draft.tasks.push(frontend);
    let plan = fixture.store.save_conversation_plan(
        &snapshot.rounds[0].id, &snapshot.messages.last().unwrap().id,
        &json!({"summary":draft.summary,"tasks":draft.tasks}).to_string(),
    ).unwrap();
    let tasks = fixture.store.apply_conversation_plan(&ConversationExecutePlanInput {
        project_id: fixture.project.clone(), plan_id: plan.id, client_request_id: "delegation-test".into(),
    }).unwrap();
    let claim = |task_id: &str, agent_id: &str| fixture.store.claim_task(ClaimTaskInput {
        task_id: task_id.into(), agent_id: agent_id.into(), lease_seconds: 120,
    });
    let start = |run: &crate::multi_agent::AgentRun| fixture.store.update_run(UpdateRunInput {
        run_id: run.id.clone(), status: RunStatus::Running, output_summary: None, error: None, session_locator: None,
    }).unwrap();
    let original = start(&claim(&tasks[0], &fixture.agents[0]).unwrap());
    let delegation_input = |target: &str| serde_json::from_value(json!({
        "targetAgentId":target,"title":"登录接口设计","brief":"明确登录返回值",
        "criteria":["成功与失败的返回值","错误码"],"checkpoint":"已完成输入校验，下一步实现返回值"
    })).unwrap();
    let unrelated = start(&claim(&tasks[1], &parallel.id).unwrap());
    assert!(fixture.store.request_design(&original.id, &fixture.agents[0], delegation_input(&fixture.agents[0])).is_err());
    assert_eq!(fixture.store.design_recipients(&tasks[0], &fixture.agents[0]).unwrap().len(), 2);
    let delegation = fixture.store.request_design(&original.id, &fixture.agents[0], delegation_input(&fixture.agents[1])).unwrap();
    assert_eq!(delegation, fixture.store.request_design(&original.id, &fixture.agents[0], delegation_input(&fixture.agents[1])).unwrap());
    assert!(!fixture.store.snapshot(&fixture.project).unwrap().tasks.iter().any(|task| task.parent_id.as_deref() == Some(&tasks[0])));
    let source = fixture.store.agent_profile(&fixture.agents[0]).unwrap();
    fixture.store.checkpoint_design_request(&original.id, &source, "backend-worktree", "checkpoint",
        &json!({"sessionId":"original-session"})).unwrap();
    let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
    let state = reopened.snapshot(&fixture.project).unwrap();
    let child = state.tasks.iter().find(|task| task.parent_id.as_deref() == Some(&tasks[0])).unwrap();
    assert_eq!(child.workspace_mode, WorkspaceMode::ReadOnly);
    assert_eq!(state.tasks.iter().find(|task| task.id == tasks[0]).unwrap().status, TaskStatus::Blocked);
    assert_eq!(state.runs.iter().find(|run| run.id == unrelated.id).unwrap().status, RunStatus::Running);
    assert!(claim(&tasks[0], &source.id).is_err());
    let design = start(&claim(&child.id, &fixture.agents[1]).unwrap());
    let delivery_input = |met| serde_json::from_value(json!({
        "filename":"登录接口契约.md","markdown":"# 登录接口\n\n成功为 200，失败为 401。",
        "acceptanceEvidence":[{"criterion":"成功与失败的返回值","evidence":"接口响应章节","met":true},
            {"criterion":"错误码","evidence":"错误码一节","met":met}]
    })).unwrap();
    assert!(reopened.submit_design(&design.id, &source.id, delivery_input(true)).is_err());
    assert!(reopened.submit_design(&design.id, &fixture.agents[1], delivery_input(false)).is_err());
    assert!(reopened.settle_design(&design.id, &fixture.agents[1], "", &json!({})).is_err());
    assert!(reopened.design_delivery(&fixture.project, &conversation, &delegation).is_err());
    assert!(claim(&tasks[0], &source.id).is_err());
    reopened.submit_design(&design.id, &fixture.agents[1], delivery_input(true)).unwrap();
    assert!(claim(&tasks[0], &source.id).is_err());
    reopened.settle_design(&design.id, &fixture.agents[1], "接口设计已提交。", &json!({"sessionId":"design-session"})).unwrap();
    let accepted = reopened.design_delivery(&fixture.project, &conversation, &delegation).unwrap();
    assert!(serde_json::to_string(&accepted).unwrap().contains("登录接口契约.md"));
    assert!(reopened.design_delivery("wrong-project", &conversation, &delegation).is_err());
    assert!(reopened.design_delivery(&fixture.project, "other-conversation", &delegation).is_err());
    let task = reopened.snapshot(&fixture.project).unwrap().tasks.into_iter().find(|task| task.id == tasks[0]).unwrap();
    assert_eq!(task.status, TaskStatus::Ready);
    let mut wrong_permission = source.clone();
    wrong_permission.permission_profile = PermissionProfile::Default;
    assert!(reopened.pending_design_checkpoint(&task, &wrong_permission).is_err());
    assert_eq!(reopened.pending_design_checkpoint(&task, &source).unwrap().unwrap().head, "checkpoint");
    assert_eq!(reopened.latest_session_locator(&task.id).unwrap().unwrap()["sessionId"], "original-session");
    assert!(reopened.design_continuation_context(&task.id).unwrap().contains("失败为 401"));
    let resumed = start(&claim(&task.id, &source.id).unwrap());
    reopened.mark_design_resumed(&resumed.id, &task.id).unwrap();
    assert!(claim(&task.id, &source.id).is_err());
    let view = serde_json::to_value(reopened.conversation_snapshot(&fixture.project).unwrap()).unwrap();
    assert_eq!(view["delegations"][0]["status"], "resumed");
    assert_eq!(view["delegations"][0]["resumeRunId"], resumed.id);
    assert_eq!(view["delegations"][0]["filename"], "登录接口契约.md");
}

#[test]
fn run_log_history_finds_old_output_outside_project_snapshot_and_checks_project() {
    let fixture = Fixture::new();
    fixture.run(&Replies::default());
    let snapshot = fixture.store.snapshot(&fixture.project).unwrap();
    let old = &snapshot.runs[0];
    let other = &snapshot.runs[1];
    fixture.store.append_run_log(&old.id, "launch_mode", "实际：Auto（Claude Code CLI）").unwrap();
    fixture.store.append_run_log(&old.id, "stdout", "older saved output").unwrap();
    for index in 0..101 {
        fixture.store.append_run_log(&other.id, "stdout", &format!("newer output {index}")).unwrap();
    }
    assert!(fixture.store.snapshot(&fixture.project).unwrap().run_logs.iter().all(|item| item.run_id != old.id));
    let old_logs = fixture.store.logs_for_run(&fixture.project, &old.id, 200).unwrap();
    assert_eq!(old_logs.last().unwrap().content, "older saved output");
    assert!(old_logs.iter().all(|item| item.run_id == old.id));
    let limited = fixture.store.logs_for_run(&fixture.project, &old.id, 1).unwrap();
    assert_eq!(limited.len(), 2);
    assert_eq!(limited[0].stream, "launch_mode");
    assert_eq!(limited[1].content, "older saved output");
    assert!(fixture.store.logs_for_run("another-project", &old.id, 200).is_err());
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
        .interrupt_conversation(ConversationInterruptInput { native_capabilities: None,
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
fn failed_plan_retry_preserves_original_request_and_requires_current_conversation() {
    let fixture = Fixture::new();
    let mut original = fixture.input();
    original.mode = "execution_plan".into();
    original.body = "规划实验目录的导出功能".into();
    original.max_duration_seconds = 125;
    original.task_rules = Some(ConversationTaskRules { tools: "none".into(), instructions: "先提出分工".into(), archived_rule_id: None });
    validate_send_input(&mut original).unwrap();
    let (round, _) = fixture.store.begin_conversation(original.clone()).unwrap();
    let get_retry = |conversation_id: &str| fixture.store.conversation_plan_retry_input(
        &fixture.project, conversation_id, &round.id, "fresh-plan-request".into(),
    );
    assert!(get_retry(&round.conversation_id).is_err(), "active plans must not restart");
    fixture.store.finish_conversation(&round.id, "needs_decision", Some("计划未通过校验".into())).unwrap();
    assert!(get_retry("another-conversation").is_err());
    let retry = get_retry(&round.conversation_id).unwrap();
    original.client_message_id = "fresh-plan-request".into();
    original.expected_conversation_id = Some(round.conversation_id.clone());
    assert_eq!(retry, original);
    assert_eq!(fixture.store.conversation_snapshot(&fixture.project).unwrap().rounds.len(), 1);
    let (retry_round, fresh) = fixture.store.begin_conversation(retry.clone()).unwrap();
    assert!(fresh && retry_round.id != round.id);
    assert!(!fixture.store.begin_conversation(retry).unwrap().1);
    let draft = fixture.draft();
    fixture.store.save_conversation_plan(&retry_round.id, &retry_round.user_message_id,
        &json!({ "summary": draft.summary, "tasks": draft.tasks }).to_string()).unwrap();
    assert!(get_retry(&round.conversation_id).is_err(), "older failures must not replace a pending plan");
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
        .control(&fixture.store, &fixture.project, "pause", None)
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
fn resumed_delivery_replaces_an_empty_commit_before_releasing_the_review() {
    let fixture = Fixture::new();
    fixture.git();
    let project = fixture.root.join("project");
    let workspace = fixture.root.join("development");
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git").arg("-C").arg(&project).args(args).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    };
    git(&["worktree", "add", "-b", "development", workspace.to_str().unwrap()]);
    let base = super::super::executor::git_head(&workspace).unwrap();
    let snapshot = fixture.run(&Replies::default());
    let draft = fixture.draft();
    let plan = fixture.store.save_conversation_plan(
        &snapshot.rounds[0].id, &snapshot.messages.last().unwrap().id,
        &json!({"summary":draft.summary,"tasks":draft.tasks}).to_string(),
    ).unwrap();
    let task = fixture.store.apply_conversation_plan(&ConversationExecutePlanInput {
        project_id: fixture.project.clone(), plan_id: plan.id, client_request_id: "resumed-delivery".into(),
    }).unwrap().remove(0);
    let review = fixture.store.create_task(serde_json::from_value::<CreateTaskInput>(json!({
        "projectId":fixture.project, "title":"review", "acceptanceCriteria":"check the actual delivery",
        "assigneeAgentId":fixture.agents[1], "workspaceMode":"read_only",
    })).unwrap()).unwrap();
    fixture.store.add_dependency(crate::multi_agent::AddDependencyInput {
        task_id: review.id.clone(), depends_on_task_id: task.clone(),
    }).unwrap();
    let run = fixture.store.claim_task(ClaimTaskInput {
        task_id: task.clone(), agent_id: fixture.agents[0].clone(), lease_seconds: 120,
    }).unwrap();
    fixture.store.update_run(UpdateRunInput {
        run_id: run.id.clone(), status: RunStatus::Running,
        output_summary: None, error: None, session_locator: None,
    }).unwrap();
    fixture.store.finalize_successful_run(&run.id, workspace.to_str().unwrap(), "write refused", false, Some(&base), None).unwrap();
    assert!(fixture.store.approve_task(&task).unwrap_err().to_string().contains("没有实际改动"));

    // The previous product could already have accepted this empty checkpoint.
    {
        let connection = fixture.store.connection.lock().unwrap();
        connection.execute("UPDATE ma_tasks SET status='done' WHERE id=?1", [&task]).unwrap();
        connection.execute("UPDATE ma_approvals SET status='approved' WHERE task_id=?1", [&task]).unwrap();
        connection.execute("UPDATE ma_tasks SET status='ready' WHERE id=?1", [&review.id]).unwrap();
    }
    std::fs::create_dir_all(workspace.join("src/owned")).unwrap();
    std::fs::write(workspace.join("src/owned/game.js"), "const ready = true;").unwrap();
    std::fs::write(workspace.join("outside"), "do not commit").unwrap();
    assert!(fixture.store.update_resumed_delivery(&task).unwrap_err().to_string().contains("超出"));
    assert_eq!(super::super::executor::git_head(&workspace).unwrap(), base);
    std::fs::remove_file(workspace.join("outside")).unwrap();
    assert_eq!(fixture.store.update_resumed_delivery(&task).unwrap().status, TaskStatus::InReview);
    let commit = super::super::executor::git_head(&workspace).unwrap();
    assert_ne!(base, commit);
    assert_eq!(super::super::executor::git_head(&project).unwrap(), base);
    let snapshot = fixture.store.snapshot(&fixture.project).unwrap();
    assert!(snapshot.approvals.iter().any(|a| a.run_id.as_deref() == Some(run.id.as_str()) && a.status == "pending"));
    assert!(snapshot.approvals.iter().any(|a| a.run_id.as_deref() == Some(run.id.as_str()) && a.status == "approved"));
    assert_eq!(snapshot.tasks.iter().find(|t| t.id == review.id).unwrap().status, TaskStatus::Blocked);
    assert_eq!(fixture.store.approve_task(&task).unwrap().status, TaskStatus::Done);
    assert!(fixture.store.runnable_tasks(&fixture.project).unwrap().iter().any(|(t, _)| t.id == review.id));
    assert!(fixture.store.dependency_artifacts(&review.id).unwrap().iter().any(|a| {
        a.run_id == run.id && a.content_hash.as_deref() == Some(commit.as_str())
    }));
    fixture.store.claim_task(ClaimTaskInput {
        task_id: review.id, agent_id: fixture.agents[1].clone(), lease_seconds: 120,
    }).unwrap();
    assert!(fixture.store.update_resumed_delivery(&task).unwrap_err().to_string().contains("后续任务已开始"));
}

fn managed_plan_fixture() -> (Fixture, ConversationSendInput, String) {
    let fixture = Fixture::at(std::env::temp_dir().join(format!("cv-{}", &id()[..8])));
    fixture.git();
    let agent = |name: &str, runtime: &str, role: &str| fixture.store.create_agent(
        serde_json::from_value(json!({"projectId":fixture.project,"name":name,"runtimeKind":runtime,"role":role})).unwrap()
    ).unwrap().id;
    let reviewer = agent("Reviewer", "claude_code", "reviewer");
    let integrator = agent("Integrator", "codex", "integrator");
    let mut input = fixture.input();
    input.participant_agent_ids.extend([reviewer.clone(), integrator.clone()]);
    input.managed = true;
    input.mode = "execution_plan".into();
    input.body = "开发并交付可运行的演示项目".into();
    let mut draft = fixture.draft();
    let implementation = &mut draft.tasks[0];
    implementation.verification_command = Some("node --test src/owned/check.test.js".into());
    let review = ConversationPlannedTask { key:"review".into(), title:"review".into(),
        assignee_agent_id:reviewer, workspace_mode:WorkspaceMode::ReadOnly, write_paths:vec![],
        verification_command:None, depends_on:vec![implementation.key.clone()], ..implementation.clone() };
    let integration = ConversationPlannedTask { key:"integration".into(), title:"integration".into(),
        assignee_agent_id:integrator, write_paths:vec!["README.md".into(),"tests".into()],
        verification_command:Some("node --test tests/acceptance.test.js".into()),
        depends_on:vec![review.key.clone()], ..implementation.clone() };
    draft.tasks.extend([review,integration]);
    (fixture, input, json!({"summary":draft.summary,"tasks":draft.tasks}).to_string())
}

#[test]
fn managed_plan_requires_all_roles_and_accepts_zcode_cli_static_review() {
    let (fixture, mut input, plan_json) = managed_plan_fixture();
    let plan: serde_json::Value = serde_json::from_str(&plan_json).unwrap();
    let reviewer = plan["tasks"][1]["assigneeAgentId"].as_str().unwrap();
    fixture.store.connection.lock().unwrap().execute(
        "UPDATE ma_agents SET runtime_kind='claude_code' WHERE id=?1", [&fixture.agents[0]],
    ).unwrap();
    input.body = "交付网页并安排只读审查".into();
    let set_role = |role| {
        fixture.store.connection.lock().unwrap().execute(
            "UPDATE ma_agents SET runtime_kind='zcode_cli', role=?2 WHERE id=?1", [reviewer, role],
        ).unwrap();
    };
    crate::runtime_preferences::with_launch_default_for_test("claude_code", "auto", || {
        set_role("worker");
        assert!(fixture.store.conversation_plan_preflight(&input).unwrap().iter().any(|issue| issue.message.contains("独立只读审查成员")));
        set_role("reviewer");
        let issues = fixture.store.conversation_plan_preflight(&input).unwrap();
        assert!(issues.is_empty(), "{:?}", issues.iter().map(|issue| &issue.message).collect::<Vec<_>>());
        let chat = fixture.run_input(input, &ScriptedReplies::new(vec![plan_json]));
        let accepted = &chat.execution_plans[0];
        assert!(accepted.managed.is_some());
        assert_eq!(accepted.tasks[1].workspace_mode, WorkspaceMode::ReadOnly);
        assert_eq!(accepted.tasks[1].assignee_agent_id, reviewer);
    });
}

#[test]
fn managed_planner_uses_selected_capabilities_without_requiring_claude() {
    let (fixture, mut input, _) = managed_plan_fixture();
    let coordinator = fixture.store.create_agent(serde_json::from_value(json!({"projectId":fixture.project,
        "name":"协调员","runtimeKind":"codex","role":"orchestrator"})).unwrap()).unwrap().id;
    fixture.store.connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind='pi' WHERE project_id=?1 AND role='reviewer'", [&fixture.project]).unwrap();
    input.participant_agent_ids.push(coordinator.clone());
    let issues = fixture.store.conversation_plan_preflight(&input).unwrap();
    assert!(issues.is_empty(), "{issues:?}");
    let (round, _) = fixture.store.begin_conversation(input).unwrap();
    assert_eq!(round.managed_target.as_ref().unwrap().planner, coordinator);
    let turn = prepare_turn(&fixture.store, &round.id, 0, "plan", 1, &coordinator).unwrap();
    assert!(fixture.store.managed_planner_target(&turn.task_id, &coordinator).unwrap().is_some());
    assert!(fixture.store.managed_planner_target(&turn.task_id, &fixture.agents[0]).is_err());
    use super::super::managed_delivery::ManagedUse;
    for role in [ManagedUse::Plan, ManagedUse::Review, ManagedUse::Write] {
        assert!(!role.supported(AgentRuntimeKind::DeepSeekHarness));
        assert!(!role.supported(AgentRuntimeKind::ZCode));
    }
    assert!(ManagedUse::Write.authorize(AgentRuntimeKind::Pi, Some("read_tools")).is_err());
}

#[test]
fn managed_restart_resumes_authorized_unclaimed_tasks_and_holds_uncertain_or_changed_grants() {
    use crate::multi_agent::ClaimTaskInput;
    for situation in ["unclaimed","interrupted","new-role","branch-moved"] {
        let (fixture,input,raw) = managed_plan_fixture();
        let chat = fixture.run_input(input,&ScriptedReplies::new(vec![raw]));
        let plan = &chat.execution_plans[0];
        let store = &fixture.store;
        let tasks = store.apply_conversation_plan_in(&ConversationExecutePlanInput { project_id:fixture.project.clone(),
            plan_id:plan.id.clone(),client_request_id:format!("managed-{}",plan.id) },true).unwrap();
        store.set_project_status(&fixture.project,ProjectStatus::Active).unwrap();
        match situation {
            "interrupted" => {
                let run = store.claim_task(ClaimTaskInput { task_id:tasks[0].clone(),agent_id:fixture.agents[0].clone(),
                    lease_seconds:120 }).unwrap();
                store.update_run(UpdateRunInput {run_id:run.id,status:RunStatus::Running,error:None,output_summary:None,
                    session_locator:Some(json!({"adapter":"codex","managedTurn":"dispatched","managedPromptDispatched":true}))}).unwrap();
                store.recover_interrupted_runs_at_startup().unwrap();
            }
            "new-role" => {
                store.connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind='pi' WHERE id=?1",[&fixture.agents[0]]).unwrap();
            }
            "branch-moved" => {
                let output = std::process::Command::new("git").current_dir(fixture.root.join("project"))
                    .args(["-c","user.name=Fixture","-c","user.email=fixture@example.invalid","commit","--allow-empty","-m","changed"]).output().unwrap();
                assert!(output.status.success());
            }
            _ => {}
        }
        let reopened = MultiAgentStore::open_at(fixture.root.join("store.db")).unwrap();
        let resume = reopened.recover_managed_deliveries_at_startup().unwrap();
        let current = reopened.conversation_snapshot(&fixture.project).unwrap().execution_plans.remove(0);
        if situation == "unclaimed" {
            assert_eq!(resume,[fixture.project.clone()]);
            assert_eq!(current.managed.unwrap().status,"running");
        } else {
            assert!(resume.is_empty(),"{situation}");
            assert_eq!(current.managed.unwrap().status,"failed","{situation}");
        }
    }
}

#[test]
fn managed_plan_retries_validation_and_delivers_only_after_review_and_tests() {
    use crate::multi_agent::executor::{commit_task_worktree, git_head, prepare_workspace_in, verify_task_acceptance};
    use crate::multi_agent::project_preview::{DeliveryDemo, PreviewStart, ProjectPreviews, MANIFEST};
    use crate::multi_agent::{RequestApprovalInput, ReviewVerdict, SubmitReviewDecisionInput};
    let (fixture, input, plan_json) = managed_plan_fixture();
    let mut draft: serde_json::Value = serde_json::from_str(&plan_json).unwrap();
    draft["tasks"][2]["writePaths"].as_array_mut().unwrap().extend([json!(MANIFEST), json!("server.js"), json!("public")]);
    let replies = ScriptedReplies::new(vec!["invalid plan".into(),draft.to_string()]);
    let chat = fixture.run_input(input, &replies);
    assert_eq!(replies.calls.lock().unwrap().len(), 2, "invalid draft automatically retried");
    assert_eq!(chat.rounds[0].status, "completed");
    let store = &fixture.store;
    let plan = &chat.execution_plans[0];
    let mut execute = ConversationExecutePlanInput {
        project_id: fixture.project.clone(), plan_id: plan.id.clone(), client_request_id: "manual-button".into(),
    };
    assert!(store.apply_conversation_plan(&execute).is_err());
    execute.client_request_id = format!("managed-{}", plan.id);
    assert!(store.apply_conversation_plan(&execute).is_err());
    let ids = store.apply_conversation_plan_in(&execute, true).unwrap();
    let project = store.project(&fixture.project).unwrap();
    let root = fixture.root.join("project");
    let initial = git_head(&root).unwrap();
    let plan = store.managed_plan_running(&fixture.project).unwrap().unwrap();
    let target = &plan.managed.as_ref().unwrap().target;
    let stop = AtomicBool::new(false);
    let claim = |index: usize| {
        let task = store.snapshot(&fixture.project).unwrap().tasks.into_iter().find(|task| task.id == ids[index]).unwrap();
        let run = store.claim_task(ClaimTaskInput {
            task_id: task.id.clone(), agent_id:task.assignee_agent_id.clone().unwrap(), lease_seconds:120,
        }).unwrap();
        store.update_run(UpdateRunInput {run_id:run.id.clone(), status:RunStatus::Running, output_summary:None,error:None,session_locator:None}).unwrap();
        let dependencies = store.dependency_artifacts(&task.id).unwrap();
        let worktree = prepare_workspace_in(&fixture.root.join("w"), &project, &task, &run.id, &dependencies).unwrap();
        (task,run,worktree)
    };
    let write = |path: &std::path::Path, value: &str| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path,value).unwrap();
    };
    let test_script = "const test = require('node:test'); test('it runs', () => require('node:assert').equal(1+1,2));";
    let finish = |run: &crate::multi_agent::AgentRun, path: &std::path::Path, verified:bool, hash:Option<&str>| {
        store.finalize_successful_run(&run.id,path.to_str().unwrap(),"verified output",verified,hash,None).unwrap()
    };
    let source = "src/owned/check.test.js";
    let reworked = format!("{test_script}\n// Reworked after review");
    for (verdict, contents) in [(ReviewVerdict::ChangesRequested,test_script), (ReviewVerdict::Approved,reworked.as_str())] {
        let (task, run, path) = claim(0);
        assert!(store.request_approval(RequestApprovalInput {
            project_id:fixture.project.clone(),task_id:Some(task.id.clone()),run_id:Some(run.id.clone()),
            approval_type:"runtime_permission".into(),summary:"extra scope".into(),
        }).is_err());
        write(&path.join(source), contents);
        assert!(verify_task_acceptance(&task,&path,&stop,Some((store,&run.id))).unwrap());
        let hash = commit_task_worktree(&path,&task.title).unwrap();
        finish(&run,&path,true,Some(&hash));
        let mut artifacts = store.dependency_artifacts(&ids[1]).unwrap();
        let receipt = serde_json::to_value(store.managed_acceptance_receipts(&artifacts).unwrap()).unwrap();
        assert_eq!(receipt[0]["commit"], hash);
        assert_eq!(receipt[0]["runId"], run.id);
        assert_eq!(receipt[0]["acceptanceVerified"], true);
        assert_eq!(receipt[0]["verificationCommand"], task.verification_command.as_deref().unwrap());
        assert!(receipt[0]["outputTail"].as_str().unwrap().contains("it runs"));
        artifacts[0].content_hash = Some("0000000000000000000000000000000000000000".into());
        assert!(store.managed_acceptance_receipts(&artifacts).unwrap().is_empty());
        let (_,review,checked) = claim(1);
        assert_eq!(std::fs::read_to_string(checked.join(source)).unwrap().replace("\r\n", "\n"), contents);
        store.submit_review_decision(SubmitReviewDecisionInput {
            review_run_id:review.id.clone(),target_task_id:task.id,verdict,summary:"review verdict".into(),
        }).unwrap();
        finish(&review,&checked,false,None);
    }
    assert_eq!(git_head(&root).unwrap(), initial);
    let (task, failed_run, failed_path) = claim(2);
    write(&failed_path.join("tests/acceptance.test.js"), "require('node:test')('fails', () => { throw Error('test regression'); });");
    let error = verify_task_acceptance(&task, &failed_path, &stop, Some((store, &failed_run.id))).unwrap_err();
    store.fail_run_with_retry(&failed_run.id, &error.to_string(), None).unwrap();
    assert_eq!(git_head(&root).unwrap(), initial);
    {
        let connection = store.connection.lock().unwrap();
        connection.execute("UPDATE ma_tasks SET next_retry_at=0 WHERE id=?1 AND next_retry_at IS NOT NULL", [&task.id]).unwrap();
    }
    let (task,run,path) = claim(2);
    write(&path.join("README.md"), "Run the demonstration with Node.js.");
    let converter = std::env::var_os("CREWVELA_PREVIEW_TEST_CONVERTER").map(std::path::PathBuf::from);
    if let Some(source) = &converter {
        write(&path.join("server.js"), &std::fs::read_to_string(source.join("server.js")).unwrap());
        write(&path.join("public/index.html"), &std::fs::read_to_string(source.join("public/index.html")).unwrap());
        write(&path.join("tests/acceptance.test.js"), &std::fs::read_to_string(source.join("tests/integration.test.js")).unwrap());
    } else {
        write(&path.join("server.js"), r#"const server = require('node:http').createServer((_req, res) => res.end('delivered'));
server.listen(Number(process.env.PORT), '127.0.0.1', () => console.log(`http://127.0.0.1:${server.address().port}/`));"#);
        write(&path.join("tests/acceptance.test.js"), test_script);
    }
    write(&path.join(MANIFEST), r#"{"program":"node","args":["server.js"],"workingDirectory":".","url":null}"#);
    let verified = verify_task_acceptance(&task,&path,&stop,Some((store,&run.id))).unwrap();
    let hash = commit_task_worktree(&path,&task.title).unwrap();
    assert!(store.finalize_successful_run(&run.id,path.to_str().unwrap(),"missing merge",verified,Some(&hash),None).is_err());
    target.merge(&project,&hash).unwrap();
    finish(&run,&path,verified,Some(&hash));
    let snapshot=store.snapshot(&fixture.project).unwrap();
    assert!(snapshot.approvals.is_empty(), "no manual delivery approval at any stage");
    let previews = ProjectPreviews::default();
    let preview_status = || serde_json::to_value(previews.status(&fixture.project, &plan.id, true)).unwrap();
    assert_eq!(preview_status()["state"], "idle", "preview waits for verified delivery");
    store.settle_idle_managed_plan(&snapshot, &plan, &previews, &stop).unwrap();
    let ready_url = || {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let status = preview_status();
            let delivery = store.preview_delivery(&fixture.project, &plan.id, None).unwrap();
            if status["state"] == "running" && matches!(delivery.demo, Some(DeliveryDemo::Ready)) {
                break status["url"].as_str().unwrap().to_owned();
            }
            assert_ne!(status["state"], "failed", "{status}");
            assert!(Instant::now() < deadline, "preview not available: {status}");
            std::thread::sleep(Duration::from_millis(50));
        }
    };
    let url = ready_url();
    let result=store.conversation_snapshot(&fixture.project).unwrap().execution_plans.remove(0).managed.unwrap();
    assert_eq!(result.status,"completed");
    assert_eq!(result.commit.as_deref(),Some(hash.as_str()));
    assert!(root.join("README.md").is_file());
    assert!(root.join("tests/acceptance.test.js").is_file());
    assert_eq!(std::fs::read_to_string(root.join(source)).unwrap().replace("\r\n", "\n"), reworked);
    assert_eq!(store.project(&fixture.project).unwrap().status,ProjectStatus::Completed);
    let check_demo = |url: &str| {
        let script = if converter.is_some() {
            r#"const assert = require('node:assert/strict');
const { chromium } = require('playwright');
(async () => {
  const browser = await chromium.launch({ channel: 'chrome' });
  try {
    const page = await browser.newPage();
    const response = await page.goto(process.argv[1]);
    assert.equal(response.status(), 200);
    await page.locator('#value-input').fill('25');
    await page.getByRole('button', { name: '换算', exact: true }).click();
    await page.locator('#result').filter({ hasText: /77/ }).waitFor();
    assert.match(await page.locator('#result').innerText(), /25.*77/);
    console.log('Browser verified the delivered converter: 25 C -> 77 F');
  } finally { await browser.close(); }
})().catch(error => { console.error(error); process.exitCode = 1; });"#
        } else {
            "fetch(process.argv[1]).then(r => r.text()).then(body => require('node:assert/strict').equal(body, 'delivered')).catch(error => { console.error(error); process.exitCode = 1; });"
        };
        let output = std::process::Command::new("node").args(["-e", script, url])
            .current_dir(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        eprint!("{}", String::from_utf8_lossy(&output.stdout));
    };
    check_demo(&url);
    previews.stop(&fixture.project, &plan.id);
    assert_eq!(preview_status()["state"], "idle");
    let changed = "throw Error('new version is not the recorded demo');";
    write(&root.join("server.js"), changed);
    let newer = commit_task_worktree(&root, "new version").unwrap();
    assert_ne!(newer, hash);
    write(&root.join("local-only.txt"), "preserve user changes");
    assert!(matches!(previews.start(store, &fixture.project, &plan.id, None).unwrap(), PreviewStart::Status { .. }));
    check_demo(&ready_url());
    assert_eq!(git_head(&root).unwrap(), newer);
    assert_eq!(std::fs::read_to_string(root.join("local-only.txt")).unwrap(), "preserve user changes");
    previews.stop(&fixture.project, &plan.id);
}

#[test]
fn managed_plan_reports_exhaustion_without_dispatching_work() {
    let (fixture, input, _) = managed_plan_fixture();
    let replies=ScriptedReplies::new(vec!["bad format".into(),"bad format".into(),"bad format".into()]);
    let chat=fixture.run_input(input,&replies);
    assert_eq!(replies.calls.lock().unwrap().len(),3);
    assert_eq!(chat.rounds[0].status,"failed");
    assert!(chat.rounds[0].stop_reason.as_deref().unwrap().contains("3 次"));
    assert!(chat.execution_plans.is_empty());
    assert!(super::super::managed_delivery::verification("node --test ../../outside.js").is_err());
    assert!(super::super::managed_delivery::verification("npm test && exit 0").is_err());
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
    let (fixture, round, mut plan) = cross_runtime_conversation_plan();
    plan.tasks[0].write_paths = vec!["frontend/".into()];
    plan.tasks[1].write_paths = vec!["backend\\".into()];
    plan.tasks[4].write_paths = vec!["frontend/".into(), "backend/".into(), "README.md".into()];
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
    assert_eq!(stored.status, "pending");
    let task_ids = fixture
        .store
        .apply_conversation_plan(&ConversationExecutePlanInput {
            project_id: fixture.project.clone(),
            plan_id: stored.id,
            client_request_id: "approved-cross-runtime".into(),
        })
        .unwrap();
    assert_eq!(task_ids.len(), 5);
    fixture.store
        .verify_conversation_write_scope(&task_ids[0], &["frontend/index.html".into()])
        .unwrap();
    fixture.store
        .verify_conversation_write_scope(&task_ids[1], &["backend/server.js".into()])
        .unwrap();
    assert!(fixture.store
        .verify_conversation_write_scope(&task_ids[0], &["frontend-other/index.html".into()])
        .is_err());
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
        &fixture.store,
        &fixture.store.connection.lock().unwrap(),
        &round,
        "summary",
        1,
        &fixture.agents[0],
        None,
        None,
    )
    .unwrap();
    assert!(prompt.contains("runtime=claude_code | role=reviewer"));
    assert!(prompt.contains("role=integrator"));
    assert!(prompt.contains("不同于被审查"));
    assert!(prompt.contains("dependsOn 必须覆盖全部 Review"));
    assert!(prompt.contains("仅设置 reviewerAgentId 不构成独立 Review 任务"));
    assert!(prompt.contains("真实成员不足"));
}

#[path = "conversation_template_tests.rs"]
mod template_cases;
