//! Opt-in model-backed acceptance of Pi's native MCP registration in the
//! production per-launch extension, using the real RPC process and a real
//! Claude ACP peer. Normal tests never start a model or touch user sessions.
use super::*;
use crate::acp::{AcpConnection, ManagedPermissionHandler};
use crate::multi_agent::{
    AgentRole, AgentRuntimeKind, ClaimTaskInput, CollaborationBridge, CreateAgentInput,
    CreateProjectInput, CreateTaskInput, MultiAgentStore, PermissionProfile, ProjectStatus,
    WorkspaceMode,
};
use anyhow::ensure;

struct TestDeadline {
    done: Option<mpsc::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for TestDeadline {
    fn drop(&mut self) {
        if let Some(done) = self.done.take() { let _ = done.send(()); }
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

fn deadline(processes: Vec<Arc<AtomicU32>>, cancelled: Arc<AtomicBool>) -> TestDeadline {
    let (done, completion) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        if matches!(completion.recv_timeout(Duration::from_secs(240)), Err(mpsc::RecvTimeoutError::Timeout)) {
            cancelled.store(true, Ordering::Release);
            for process in processes { let _ = crate::acp::terminate_acp_process_checked(&process); }
        }
    });
    TestDeadline { done: Some(done), worker: Some(worker) }
}

fn participant(store: &MultiAgentStore, project: &str, name: &str, runtime: AgentRuntimeKind) -> Result<(String, String, String)> {
    let agent = store.create_agent(CreateAgentInput {
        project_id: project.into(), name: name.into(), runtime_kind: runtime,
        role: AgentRole::Worker, description: "isolated live communication acceptance".into(),
        persona: Default::default(), persona_custom: String::new(), avatar_id: None,
        model: None, permission_profile: PermissionProfile::Default, max_concurrency: 1,
    })?;
    let task = store.create_task(CreateTaskInput {
        project_id: project.into(), parent_id: None, stage: 0, title: format!("{name} fixture mailbox"),
        description: String::new(), acceptance_criteria: "one exact message and acknowledgement".into(),
        verification_command: None, assignee_agent_id: Some(agent.id.clone()), reviewer_agent_id: None,
        workspace_mode: WorkspaceMode::ReadOnly, priority: 0, required_capabilities: vec![],
        max_attempts: 1, retry_backoff_seconds: 0,
    })?;
    let run = store.claim_task(ClaimTaskInput { task_id: task.id.clone(), agent_id: agent.id.clone(), lease_seconds: 300 })?;
    Ok((agent.id, task.id, run.id))
}

fn permissions() -> ManagedPermissionHandler {
    Box::new(|request, _| {
        let tool = &request["params"]["toolCall"];
        let raw = &tool["rawInput"];
        let structured = raw["server"] == "crewvela"
            && matches!(raw["tool"].as_str(), Some("cc_message_send" | "cc_thread_read"));
        let named = [tool["title"].as_str(), raw["name"].as_str()].into_iter().flatten().any(|name| matches!(name,
            "mcp__crewvela__cc_message_send" | "mcp__crewvela__cc_thread_read" | "crewvela__cc_message_send" | "crewvela__cc_thread_read"));
        Ok(structured || named)
    })
}

fn pi_prompt(client: &PiClient, events: &mpsc::Receiver<Value>, prompt: &str, end: Instant) -> Result<Vec<Value>> {
    while events.try_recv().is_ok() {}
    client.command(Duration::from_secs(30), json!({"type":"prompt","message":prompt}))?;
    let mut captured = Vec::new();
    loop {
        let remaining = end.saturating_duration_since(Instant::now());
        ensure!(!remaining.is_zero(), "Pi model turn exceeded the acceptance deadline");
        let frame = events.recv_timeout(remaining.min(Duration::from_secs(1)));
        match frame {
            Ok(frame) => {
                ensure!(frame["type"] != "__acceptance_disconnected", "real Pi RPC process disconnected");
                let settled = frame["type"] == "agent_settled";
                captured.push(frame);
                if settled {
                    let assistant = captured.iter().rev().find(|event| event["type"] == "message_end" && event["message"]["role"] == "assistant")
                        .context("Pi settled without a final assistant event")?;
                    ensure!(assistant["message"]["stopReason"] == "stop", "Pi model turn ended without successful assistant completion");
                    return Ok(captured);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {},
            Err(error) => return Err(error).context("real Pi event stream closed"),
        }
    }
}

fn completed_tool(events: &[Value], name: &str) -> bool {
    events.iter().any(|event| event["type"] == "tool_execution_end"
        && event["toolName"] == name && event["isError"] == false
        && event["toolCallId"].as_str().is_some_and(|id| !id.is_empty()
            && events.iter().any(|started| started["type"] == "tool_execution_start"
                && started["toolCallId"] == id && started["toolName"] == name)))
}

#[test]
fn pi_message_receipt_requires_the_same_nonempty_successful_tool_identity() {
    let name = "mcp__crewvela__cc_thread_read";
    let start = json!({"type":"tool_execution_start","toolName":name,"toolCallId":"read-1"});
    assert!(!completed_tool(&[json!({"type":"tool_execution_end","toolName":name,"isError":false})], name));
    assert!(!completed_tool(&[start.clone(), json!({"type":"tool_execution_end","toolName":name,"toolCallId":"different","isError":false})], name));
    assert!(!completed_tool(&[start.clone(), json!({"type":"tool_execution_end","toolName":name,"toolCallId":"read-1","isError":true})], name));
    assert!(completed_tool(&[start, json!({"type":"tool_execution_end","toolName":name,"toolCallId":"read-1","isError":false})], name));
}

fn tool_evidence(events: &[Value]) -> Vec<Value> {
    events.iter().filter(|event| event["type"] == "tool_execution_start" || event["type"] == "tool_execution_end")
        .map(|event| json!({"type":event["type"],"toolCallId":event["toolCallId"],"toolName":event["toolName"],"isError":event["isError"]})).collect()
}

#[test]
#[ignore = "opt-in: CREWVELA_RUN_REAL_PI_COMMUNICATION=1; real authenticated Pi and Claude models, at most three prompts"]
fn real_pi_claude_mcp_message_roundtrip() -> Result<()> {
    ensure!(std::env::var("CREWVELA_RUN_REAL_PI_COMMUNICATION").as_deref() == Ok("1"), "explicitly enable live Pi/Claude model calls");
    let root = std::env::temp_dir().join(format!("crewvela-real-pi-communication-{}", uuid::Uuid::new_v4()));
    let pi_cwd = root.join("pi");
    let claude_cwd = root.join("claude");
    fs::create_dir_all(&pi_cwd)?;
    fs::create_dir_all(&claude_cwd)?;
    let store = MultiAgentStore::open_at(root.join("control.db"))?;
    let project = store.create_project(CreateProjectInput {
        name: "isolated live Pi communication".into(), goal: "Pi -> Claude -> Pi exact marker".into(),
        root_path: root.display().to_string(), default_branch: "main".into(),
    })?;
    store.set_project_status(&project.id, ProjectStatus::Active)?;
    let (pi_agent, pi_task, pi_run) = participant(&store, &project.id, "live-pi", AgentRuntimeKind::Pi)?;
    let (claude_agent, claude_task, claude_run) = participant(&store, &project.id, "live-claude", AgentRuntimeKind::ClaudeCode)?;
    let bridge = CollaborationBridge::start(store.clone())?;
    let pi_lease = bridge.register_run(&project.id, &pi_task, &pi_run, &pi_agent, &pi_cwd);
    let claude_lease = bridge.register_run(&project.id, &claude_task, &claude_run, &claude_agent, &claude_cwd);
    let access_pi = pi_lease.access();
    let access_claude = claude_lease.access();
    let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events"))?;
    let app_session = sessions.create_agent(crate::sessions::NewAgentSession {
        avatar_id: None, project_id: project.id.clone(), agent_id: pi_agent.clone(), runtime_kind: "pi".into(),
        agent_name: "live Pi acceptance".into(), cwd: pi_cwd.display().to_string(),
    })?;
    let executable = crate::multi_agent::resolve_runtime_executable(AgentRuntimeKind::Pi).context("installed Pi CLI unavailable")?;
    let mut command = crate::multi_agent::command_for_path(&executable);
    command.args(["--mode", "rpc", "--provider", "minimax-cn", "--model", "MiniMax-M2.7",
        "--no-extensions", "-e", "builtin:mcp", "--no-skills", "--no-prompt-templates", "--no-themes", "--no-context-files", "--no-approve",
        "--tools", "mcp__crewvela__cc_message_send,mcp__crewvela__cc_thread_read"])
        .arg("--session-dir").arg(root.join("pi-sessions"))
        .env("CREWVELA_PI_MCP_URL", format!("{}/mcp", access_pi.endpoint))
        .env("CREWVELA_AGENT_TOKEN", &access_pi.token)
        .current_dir(&pi_cwd);
    sessions.configure_pi_readiness(&mut command, &app_session.id)?;
    let (event_sender, events) = mpsc::channel();
    let disconnected = event_sender.clone();
    let pi = PiClient::spawn(command, Box::new(move |event| { let _ = event_sender.send(event); }),
        Box::new(move || { let _ = disconnected.send(json!({"type":"__acceptance_disconnected"})); }))?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let claude_pid = Arc::new(AtomicU32::new(0));
    let _deadline = deadline(vec![pi.process_id.clone(), claude_pid.clone()], cancelled.clone());
    let end = Instant::now() + Duration::from_secs(240);
    eprintln!("REAL_PI_COMMUNICATION_ROOT={}", root.display());

    let result = (|| -> Result<()> {
        let initial = pi.command(Duration::from_secs(45), json!({"type":"get_state"}))?;
        let pi_native = initial["sessionId"].as_str().filter(|id| !id.is_empty()).context("Pi omitted native session ID")?.to_string();
        ensure!(initial["model"]["id"] == "MiniMax-M2.7", "Pi did not select the expected authenticated model");
        let mcp = json!({"type":"http","name":"crewvela","url":format!("{}/mcp",access_claude.endpoint),
            "headers":[{"name":"Authorization","value":format!("Bearer {}", access_claude.token)}]});
        let mut claude = AcpConnection::spawn_native_claude_chat(&claude_cwd.display().to_string(), None, vec![mcp], permissions(), claude_pid, None)?;
        let claude_native = claude.session_id().to_string();
        let claude_tools = Arc::new(Mutex::new(Vec::new()));
        let captured = claude_tools.clone();
        claude.set_update_handler(Box::new(move |update| {
            if matches!(update["sessionUpdate"].as_str(), Some("tool_call" | "tool_call_update")) {
                captured.lock().unwrap().push(json!({"toolCallId":update["toolCallId"],"title":update["title"],
                    "status":update["status"],"isRead":update.to_string().contains("cc_thread_read")}));
            }
        }));
        let marker = format!("CREWVELA_PI_PROBE_{}", uuid::Uuid::new_v4().simple());
        let sent = pi_prompt(&pi, &events, &format!("This is an isolated communication acceptance test. Use only mcp__crewvela__cc_message_send. Send exactly {marker} as body to targetAgentId {claude_agent}. Do not use any other tools. After the tool succeeds reply SENT."), end)?;
        fs::write(root.join("pi-send-tools.json"), serde_json::to_vec_pretty(&tool_evidence(&sent))?)?;
        ensure!(completed_tool(&sent, "mcp__crewvela__cc_message_send"), "Pi did not complete the real MCP send tool");
        let snapshot = store.snapshot(&project.id)?;
        let outbound = snapshot.messages.iter().find(|message| message.body == marker).context("real Pi model did not persist its message")?;
        ensure!(outbound.sender_id.as_deref() == Some(&pi_agent) && outbound.target_agent_id.as_deref() == Some(&claude_agent)
            && outbound.task_id.as_deref() == Some(&pi_task), "Pi message identity/task changed");
        let response = claude.prompt_interactive(&format!("Isolated communication acceptance. Use only crewvela MCP cc_thread_read and cc_message_send. Read your thread, find the received body starting CREWVELA_PI_PROBE_, and send body ACK: immediately followed by that exact received body to targetAgentId {pi_agent}. Do not invent the received body or use any other tool. Reply only the received body after sending."), &cancelled)?;
        ensure!(!response.cancelled && response.text.contains(&marker), "Claude failed to read the exact Pi message");
        let snapshot = store.snapshot(&project.id)?;
        let reply = snapshot.messages.iter().find(|message| message.body.strip_prefix("ACK:").map(str::trim) == Some(marker.as_str())).context("Claude did not persist the acknowledgement")?;
        ensure!(reply.sender_id.as_deref() == Some(&claude_agent) && reply.target_agent_id.as_deref() == Some(&pi_agent)
            && reply.task_id.as_deref() == Some(&claude_task), "Claude reply identity/task changed");
        let received = pi_prompt(&pi, &events, "Use only mcp__crewvela__cc_thread_read. Read your thread, find the received acknowledgement whose body starts ACK:CREWVELA_PI_PROBE_ (there may be a space after ACK:), and reply with that exact body. Do not send any new messages or use other tools.", end)?;
        fs::write(root.join("pi-read-tools.json"), serde_json::to_vec_pretty(&tool_evidence(&received))?)?;
        ensure!(completed_tool(&received, "mcp__crewvela__cc_thread_read"), "Pi did not complete its real thread-read tool");
        ensure!(received.iter().any(|event| event["type"] == "tool_execution_end"
            && event["toolName"] == "mcp__crewvela__cc_thread_read" && event["isError"] == false
            && event["result"].to_string().contains(&reply.body)), "Pi thread-read result did not actually contain the stored acknowledgement");
        let messages = pi.command(Duration::from_secs(15), json!({"type":"get_messages"}))?;
        let assistant = messages["messages"].as_array().context("Pi messages response omitted messages")?.iter()
            .rev().find(|message| message["role"] == "assistant").context("Pi omitted the final assistant reply")?;
        let assistant_text = assistant["content"].as_array().map(|parts| parts.iter().filter_map(|part| part["text"].as_str()).collect::<Vec<_>>().join("\n")).unwrap_or_default();
        ensure!(assistant_text.contains(&marker) && assistant_text.contains("ACK:"), "Pi final assistant reply did not include the exact received acknowledgement");
        let final_state = pi.command(Duration::from_secs(15), json!({"type":"get_state"}))?;
        ensure!(final_state["sessionId"] == pi_native && claude.session_id() == claude_native, "communication changed a native session");
        ensure!(store.snapshot(&project.id)?.messages.iter().filter(|message| message.sender_type == "agent").count() == 2, "read caused a duplicate reply loop");
        let native_file = final_state["sessionFile"].as_str().context("Pi omitted persisted native session file")?;
        ensure!(Path::new(native_file).is_file(), "Pi native history was not persisted");
        fs::write(root.join("acceptance.json"), serde_json::to_vec_pretty(&json!({
            "passed":true,"prompts":3,"agentMessages":2,"marker":marker,"piNativeSessionId":pi_native,
            "claudeNativeSessionId":claude_native,"stableNativeSessions":true,"piAgentId":pi_agent,"claudeAgentId":claude_agent,
            "outboundId":outbound.id,"replyId":reply.id,"model":initial["model"]["id"],"provider":initial["model"]["provider"],
            "piSendTools":tool_evidence(&sent),"piReadTools":tool_evidence(&received),"claudeTools":*claude_tools.lock().unwrap(),
            "productionPrivateExtension":true,"globalConfigurationModified":false,"permissionBypass":false,
            "piReadResultContainsExactAcknowledgement":true,"successfulAssistantTurns":true,
        }))?)?;
        eprintln!("REAL_PI_COMMUNICATION_OK: Pi send -> Claude read/reply -> Pi read, stable native sessions, exact marker {marker}");
        Ok(())
    })();
    if let Err(error) = &result {
        fs::write(root.join("failed.json"), serde_json::to_vec_pretty(&json!({"passed":false,"error":format!("{error:#}")}))?)?;
        let diagnostic = pi.stderr_tail().lines().filter(|line| line.to_ascii_lowercase().contains("mcp") || line.to_ascii_lowercase().contains("extension"))
            .collect::<Vec<_>>().join("\n").replace(&access_pi.token, "[REDACTED]");
        fs::write(root.join("pi-mcp-diagnostic.txt"), diagnostic)?;
    }
    pi.stop_checked()?;
    result
}
