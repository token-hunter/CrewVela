use super::*;
use crate::control::{AgentActivity, SessionActionQueue};
use crate::multi_agent::{
    AgentRole, ClaimTaskInput, CreateAgentInput, CreateProjectInput, CreateTaskInput,
    PermissionProfile, WorkspaceMode,
};
use crate::sessions::{NewAgentSession, SessionManager};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn scoped_cli_and_http_mcp_deliver_one_private_turn_and_reference_only_handoffs() -> Result<()> {
    let root =
        std::env::temp_dir().join(format!("crewvela-member-purpose-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root)?;
    let db = root.join("team.db");
    let store = MultiAgentStore::open_at(db.clone())?;
    let project = store.create_project(CreateProjectInput {
        name: "消息测试".into(),
        goal: "核验实际通信入口".into(),
        root_path: root.display().to_string(),
        default_branch: "main".into(),
    })?;
    let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events"))?;
    let listener = TcpListener::bind("127.0.0.1:0")?;
    listener.set_nonblocking(true)?;
    let bridge = CollaborationBridge {
        endpoint: format!("http://{}", listener.local_addr()?),
        scopes: Default::default(),
        agent_tokens: Default::default(),
    };
    let (scopes, server_store) = (bridge.scopes.clone(), store.clone());
    let done = Arc::new(AtomicBool::new(false));
    let stop = done.clone();
    let server = std::thread::spawn(move || -> Result<()> {
        let deadline = std::time::Instant::now() + Duration::from_secs(45);
        while !stop.load(Ordering::Acquire) && std::time::Instant::now() < deadline {
            match listener.accept() {
                Ok((s, _)) => {
                    s.set_nonblocking(false)?;
                    handle_connection(s, &server_store, &scopes)?;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(2))
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    });
    let mut members = Vec::new();
    for name in ["发起人", "收件人", "旁观者"] {
        let agent = store.create_agent(CreateAgentInput {
            project_id: project.id.clone(),
            name: name.into(),
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
        let task = store.create_task(CreateTaskInput {
            project_id: project.id.clone(),
            parent_id: None,
            stage: 0,
            title: name.into(),
            description: String::new(),
            acceptance_criteria: "核验投递".into(),
            verification_command: None,
            assignee_agent_id: Some(agent.id.clone()),
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::ReadOnly,
            priority: 0,
            required_capabilities: vec![],
            max_attempts: 1,
            retry_backoff_seconds: 5,
        })?;
        let run = store.claim_task(ClaimTaskInput {
            task_id: task.id.clone(),
            agent_id: agent.id.clone(),
            lease_seconds: 120,
        })?;
        let session = sessions.create_agent(NewAgentSession {
            project_id: project.id.clone(),
            agent_id: agent.id.clone(),
            agent_name: name.into(),
            runtime_kind: "codex".into(),
            avatar_id: None,
            cwd: root.display().to_string(),
        })?;
        sessions.set_runtime_command_host(&session.id, true)?;
        store.link_agent_app_session(&project.id, &agent.id, &session.id)?;
        let lease = bridge.register_run(&project.id, &task.id, &run.id, &agent.id, &root);
        members.push((agent, task, run, session, lease));
    }
    let (sender, source, run, session, lease) = &members[0];
    let (receiver, receiver_task, _, target, _) = &members[1];
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let client = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(4))
        .build()?;
    let call = |member: usize, name: &str, args: Value| -> Result<Value> {
        let response:Value=runtime.block_on(async {
            client.post(format!("{}/mcp",bridge.endpoint)).bearer_auth(&members[member].4.token)
                .json(&json!({"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":name,"arguments":args}}))
                .send().await?.json().await
        })?;
        Ok(response["result"].clone())
    };
    let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox"))?;
    let request_id = uuid::Uuid::new_v4().to_string();
    let status = json!({"targetAgentId":receiver.id,"body":"构建已完成","purpose":"status","requestId":request_id});
    let first = call(0, "cc_message_send", status.clone())?;
    assert_eq!(first["structuredContent"]["stage"], "inbox", "{first}");
    assert_eq!(
        first["structuredContent"],
        call(0, "cc_message_send", status)?["structuredContent"]
    );
    assert!(store.drain_member_inbox(&queue)?.is_empty());
    assert_eq!(store.snapshot(&project.id)?.tasks.len(), 3);
    assert!(call(1, "cc_thread_read", json!({}))?
        .to_string()
        .contains("构建已完成"));
    assert!(!call(2, "cc_thread_read", json!({}))?
        .to_string()
        .contains("构建已完成"));
    for bad in [
        json!({"purpose":"question","body":"少了收件人"}),
        json!({"purpose":"question","targetAgentId":sender.id,"body":"自发"}),
        json!({"purpose":"question","targetAgentId":receiver.id,"sourceTaskId":"forged","body":"伪造来源"}),
        json!({"purpose":"handoff","targetAgentId":receiver.id,"body":"缺少证据"}),
        json!({"purpose":"handoff","targetAgentId":receiver.id,"body":"伪造证据","handoff":{"todos":["核验"],"acceptance":["通过"],"artifactIds":["other-task"]}}),
    ] {
        assert_eq!(call(0, "cc_message_send", bad)?["isError"], true);
    }
    let file = root.join("full.log");
    std::fs::write(&file, "原始明细不可自动转发".repeat(4000))?;
    let artifact = call(
        0,
        "cc_artifact_publish",
        json!({"artifactType":"log","uri":"full.log","summary":"完整验证记录"}),
    )?;
    let artifact_id = artifact["structuredContent"]["id"]
        .as_str()
        .context("缺少产物回执")?;
    let arg = json!({"purpose":"handoff","targetAgentId":receiver.id,"body":"审核构建结果","handoff":{
        "todos":["核验测试"],"acceptance":["记录可复现"],"artifactIds":[artifact_id]}});
    let handoff = call(0, "cc_message_send", arg)?;
    let handoff_id = handoff["structuredContent"]["messageId"]
        .as_str()
        .context("缺少交接消息")?;
    assert_eq!(
        call(1, "cc_artifact_inspect", json!({"artifactId":artifact_id}))?["isError"],
        false
    );
    assert_eq!(
        call(2, "cc_artifact_inspect", json!({"artifactId":artifact_id}))?["isError"],
        true
    );
    std::fs::write(root.join(".cli-fixture"), "test-only")?;
    let mut child = std::process::Command::new(std::env::current_exe()?);
    child
        .args([
            "--exact",
            "messaging::tests::isolated_cli_child_process",
            "--nocapture",
        ])
        .env("CREWVELA_MESSAGE_TEST_ROOT", &root)
        .env("CREWVELA_SESSION_ID", &session.id)
        .env(
            "CREWVELA_MESSAGE_TEST_ARGS",
            json!([
                "send",
                "--to",
                target.id,
                "--body",
                "请核实结果",
                "--purpose",
                "question"
            ])
            .to_string(),
        );
    crate::messaging::configure_member_cli(&mut child, &bridge.endpoint, &lease.token, &run.id);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        child.creation_flags(0x08000000);
    }
    let output = child.output()?;
    assert!(
        output.status.success(),
        "{} {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("\"queued\""));
    queue.set_activity(&target.id, AgentActivity::Muted);
    assert!(store.drain_member_inbox(&queue)?.is_empty());
    queue.set_activity(&target.id, AgentActivity::Active);
    let reopened = MultiAgentStore::open_at(db)?;
    assert!(reopened.drain_member_inbox(&queue)?.contains(&project.id));
    reopened.drain_member_inbox(&queue)?;
    let snapshot = reopened.snapshot(&project.id)?;
    let turns: Vec<_> = snapshot
        .tasks
        .iter()
        .filter(|t| t.id.starts_with("cli-message-"))
        .collect();
    assert_eq!(turns.len(), 2);
    assert!(turns
        .iter()
        .all(|t| t.assignee_agent_id.as_ref() == Some(&receiver.id)));
    let job = turns
        .iter()
        .find(|t| t.id == format!("cli-message-{handoff_id}"))
        .unwrap();
    for expected in [
        "审核构建结果",
        "核验测试",
        "记录可复现",
        artifact_id,
        &source.id,
        handoff_id,
    ] {
        assert!(job.description.contains(expected));
    }
    assert!(!job.description.contains("原始明细不可自动转发"));
    assert!(
        !reopened
            .runnable_tasks(&project.id)?
            .iter()
            .any(|(t, _)| t.id == job.id),
        "recipient is already running its own job"
    );
    assert!(reopened
        .scoped_messages(&project.id, &receiver_task.id, &receiver.id, 100)?
        .iter()
        .any(|m| m.id == handoff_id));
    done.store(true, Ordering::Release);
    server.join().unwrap()?;
    drop(reopened);
    drop(store);
    drop(sessions);
    drop(queue);
    std::fs::remove_dir_all(root)?;
    Ok(())
}
