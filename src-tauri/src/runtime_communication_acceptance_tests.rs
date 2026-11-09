//! Explicit, model-backed communication matrix using production ACP adapters.
//! No normal test run starts models; opt in to this exact ignored test.

use crate::acp::{AcpConnection, ManagedPermissionHandler};
use crate::multi_agent::{
    AgentRole, AgentRuntimeKind, ClaimTaskInput, CollaborationBridge, CreateAgentInput,
    CreateProjectInput, CreateTaskInput, MultiAgentStore, PermissionProfile, ProjectStatus,
    WorkspaceMode,
};
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    mpsc, Arc, Mutex,
};
use std::time::Duration;

struct ModelParticipant {
    runtime: AgentRuntimeKind,
    agent_id: String,
    task_id: String,
    native_session_id: String,
    model_selection: Value,
    connection: AcpConnection,
    tools: Arc<Mutex<Vec<Value>>>,
}

struct AcceptanceDeadline {
    finished: Option<mpsc::Sender<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for AcceptanceDeadline {
    fn drop(&mut self) {
        if let Some(finished) = self.finished.take() {
            let _ = finished.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn deadline(cancelled: &Arc<AtomicBool>, processes: &[Arc<AtomicU32>]) -> AcceptanceDeadline {
    let (finished, completion) = mpsc::channel();
    let cancelled = cancelled.clone();
    let processes = processes.to_vec();
    let worker = std::thread::spawn(move || {
        if matches!(
            completion.recv_timeout(Duration::from_secs(600)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ) {
            cancelled.store(true, Ordering::Release);
            for process in processes {
                let _ = crate::acp::terminate_acp_process_checked(&process);
            }
        }
    });
    AcceptanceDeadline {
        finished: Some(finished),
        worker: Some(worker),
    }
}

fn communication_permissions(denied: Arc<Mutex<Vec<Value>>>) -> ManagedPermissionHandler {
    Box::new(move |request, _| {
        let tool = &request["params"]["toolCall"];
        let raw = &tool["rawInput"];
        let structured = raw["server"] == "crewvela"
            && matches!(
                raw["tool"].as_str(),
                Some("cc_message_send" | "cc_thread_read")
            );
        let named = [tool["title"].as_str(), raw["name"].as_str()]
            .into_iter()
            .flatten()
            .any(|name| {
                matches!(
                    name,
                    "mcp__crewvela__cc_message_send"
                        | "mcp__crewvela__cc_thread_read"
                        | "crewvela_cc_message_send"
                        | "crewvela_cc_thread_read"
                        | "crewvela__cc_message_send"
                        | "crewvela__cc_thread_read"
                        | "cc_message_send"
                        | "cc_thread_read"
                )
            });
        if !(structured || named) {
            denied
                .lock()
                .unwrap()
                .push(json!({"title": tool["title"], "kind": tool["kind"]}));
        }
        Ok(structured || named)
    })
}

fn mcp_server(endpoint: &str, token: &str) -> Value {
    json!({"type":"http", "name":"crewvela", "url":format!("{endpoint}/mcp"),
        "headers":[{"name":"Authorization", "value":format!("Bearer {token}")} ]})
}

fn save(root: &Path, name: &str, value: &Value) -> Result<()> {
    std::fs::write(root.join(name), serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

fn read_completed(events: &[Value]) -> bool {
    events.iter().any(|call| {
        call["isThreadRead"] == true
            && call["toolCallId"].as_str().is_some_and(|id| {
                !id.is_empty()
                    && events
                        .iter()
                        .any(|event| event["toolCallId"] == id && event["status"] == "completed")
            })
    })
}

fn is_thread_read_call(update: &Value) -> bool {
    // Updates can omit the wrapper's title while repeating its nested input.
    // Only an initial call may establish tool identity; completion is matched
    // separately by the already identified nonempty toolCallId.
    if update["sessionUpdate"] != "tool_call" {
        return false;
    }
    let read_name = |name: &str| {
        matches!(
            name,
            "mcp__crewvela__cc_thread_read"
                | "mcp.crewvela.cc_thread_read"
                | "crewvela_cc_thread_read"
                | "crewvela__cc_thread_read"
                | "cc_thread_read"
        )
    };
    // Discovery and Guardian events may embed the requested MCP arguments.
    // Their own title identifies them as wrappers, never an executed read.
    if let Some(title) = update["title"].as_str() {
        return read_name(title);
    }
    let raw = &update["rawInput"];
    (raw["server"] == "crewvela" && raw["tool"] == "cc_thread_read")
        || raw["name"].as_str().is_some_and(read_name)
}

#[test]
fn tool_discovery_or_text_mention_is_not_a_thread_read() {
    for update in [
        json!({"sessionUpdate":"tool_call","title":"ToolSearch","rawInput":{"query":"select:mcp__crewvela__cc_thread_read"}}),
        json!({"sessionUpdate":"tool_call","title":"shell","content":[{"text":"cc_thread_read completed"}]}),
        json!({"sessionUpdate":"tool_call","title":"mcp.crewvela.cc_message_send","rawInput":{"body":"cc_thread_read"}}),
        json!({"sessionUpdate":"tool_call","title":"unrelated_cc_thread_read"}),
        json!({"sessionUpdate":"tool_call","title":"Guardian Review","rawInput":{"server":"crewvela","tool":"cc_thread_read"}}),
        json!({"sessionUpdate":"tool_call_update","toolCallId":"guardian","status":"completed","rawInput":{"server":"crewvela","tool":"cc_thread_read"}}),
    ] {
        assert!(!is_thread_read_call(&update));
    }
    assert!(is_thread_read_call(
        &json!({"sessionUpdate":"tool_call","title":"mcp__crewvela__cc_thread_read"})
    ));
    assert!(is_thread_read_call(
        &json!({"sessionUpdate":"tool_call","title":"mcp.crewvela.cc_thread_read"})
    ));
    assert!(is_thread_read_call(
        &json!({"sessionUpdate":"tool_call","rawInput":{"server":"crewvela","tool":"cc_thread_read"}})
    ));
}

#[test]
fn a_receipt_requires_the_same_nonempty_tool_identity_to_complete() {
    assert!(!read_completed(&[
        json!({"isThreadRead":true}),
        json!({"status":"completed"})
    ]));
    assert!(!read_completed(&[
        json!({"isThreadRead":true,"toolCallId":"read"}),
        json!({"toolCallId":"send","status":"completed"})
    ]));
    assert!(read_completed(&[
        json!({"isThreadRead":true,"toolCallId":"read"}),
        json!({"toolCallId":"read","status":"completed"})
    ]));
}

#[test]
#[ignore = "opt-in: real installed authenticated Runtime models; CREWVELA_RUN_REAL_RUNTIME_MATRIX=1; at most three prompts per participant"]
fn real_acp_runtime_direct_message_matrix() -> Result<()> {
    ensure!(
        std::env::var("CREWVELA_RUN_REAL_RUNTIME_MATRIX").as_deref() == Ok("1"),
        "explicit opt-in required for real model calls"
    );
    let selected = std::env::var("CREWVELA_REAL_RUNTIME_SET")
        .unwrap_or_else(|_| "kimi_code,opencode,deepseek_harness".into());
    let runtimes = selected
        .split(',')
        .map(|kind| {
            let runtime: AgentRuntimeKind = serde_json::from_value(json!(kind.trim()))?;
            ensure!(
                matches!(
                    runtime,
                    AgentRuntimeKind::KimiCode
                        | AgentRuntimeKind::OpenCode
                        | AgentRuntimeKind::DeepSeekHarness
                        | AgentRuntimeKind::ClaudeCode
                        | AgentRuntimeKind::Codex
                ),
                "this exact test covers ACP-capable Runtime transports only"
            );
            Ok(runtime)
        })
        .collect::<Result<Vec<_>>>()?;
    ensure!(
        runtimes.len() >= 2 && runtimes.len() <= 5,
        "select two to five unique runtimes"
    );
    let unique = runtimes
        .iter()
        .map(|kind| kind.as_str())
        .collect::<std::collections::HashSet<_>>();
    ensure!(
        unique.len() == runtimes.len(),
        "duplicate Runtime would not be cross-runtime acceptance"
    );
    let root = std::env::temp_dir().join(format!(
        "crewvela-real-runtime-matrix-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir(&root)?;
    eprintln!("REAL_RUNTIME_MATRIX_ROOT={}", root.display());
    let store = MultiAgentStore::open_at(root.join("control.db"))?;
    let project = store.create_project(CreateProjectInput {
        name: "isolated real Runtime matrix".into(),
        goal: "random marker communication only".into(),
        root_path: root.display().to_string(),
        default_branch: "main".into(),
    })?;
    store.set_project_status(&project.id, ProjectStatus::Active)?;
    let bridge = CollaborationBridge::start(store.clone())?;
    let cancelled = Arc::new(AtomicBool::new(false));
    let processes = runtimes
        .iter()
        .map(|_| Arc::new(AtomicU32::new(0)))
        .collect::<Vec<_>>();
    let _deadline = deadline(&cancelled, &processes);
    let mut leases = Vec::new();
    let mut specifications = Vec::new();
    for runtime in &runtimes {
        let cwd = root.join(runtime.as_str());
        std::fs::create_dir(&cwd)?;
        let agent = store.create_agent(CreateAgentInput {
            project_id: project.id.clone(),
            name: format!("acceptance-{}", runtime.as_str()),
            runtime_kind: *runtime,
            role: AgentRole::Worker,
            description: "isolated real model communication".into(),
            persona: Default::default(),
            persona_custom: String::new(),
            avatar_id: None,
            model: None,
            permission_profile: PermissionProfile::Default,
            max_concurrency: 1,
        })?;
        let task = store.create_task(CreateTaskInput {
            project_id: project.id.clone(),
            parent_id: None,
            stage: 0,
            title: format!("{} mailbox", runtime.as_str()),
            description: String::new(),
            acceptance_criteria: "scoped messages only".into(),
            verification_command: None,
            assignee_agent_id: Some(agent.id.clone()),
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::ReadOnly,
            priority: 0,
            required_capabilities: Vec::new(),
            max_attempts: 1,
            retry_backoff_seconds: 0,
        })?;
        let run = store.claim_task(ClaimTaskInput {
            task_id: task.id.clone(),
            agent_id: agent.id.clone(),
            lease_seconds: 900,
        })?;
        let lease = bridge.register_run(&project.id, &task.id, &run.id, &agent.id, &cwd);
        let access = lease.access();
        specifications.push((
            *runtime,
            agent.id,
            task.id,
            cwd,
            mcp_server(&access.endpoint, &access.token),
        ));
        leases.push(lease);
    }
    let mut participants = Vec::new();
    let denied = Arc::new(Mutex::new(Vec::new()));
    let mut failures = Vec::<Value>::new();
    for (index, (runtime, agent_id, task_id, cwd, server)) in specifications.iter().enumerate() {
        let args = (
            cwd.display().to_string(),
            vec![server.clone()],
            processes[index].clone(),
        );
        let handler = communication_permissions(denied.clone());
        let started = match runtime {
            AgentRuntimeKind::KimiCode => {
                AcpConnection::spawn_native_kimi_chat(&args.0, None, args.1, handler, args.2, None)
            }
            AgentRuntimeKind::OpenCode => AcpConnection::spawn_native_opencode_chat(
                &args.0, None, args.1, handler, args.2, None,
            ),
            AgentRuntimeKind::DeepSeekHarness => AcpConnection::spawn_native_deepseek_harness(
                &args.0, None, args.1, handler, args.2, None,
            ),
            AgentRuntimeKind::ClaudeCode => AcpConnection::spawn_native_claude_chat(
                &args.0, None, args.1, handler, args.2, None,
            ),
            AgentRuntimeKind::Codex => {
                AcpConnection::spawn_native_codex_chat(&args.0, None, args.1, handler, args.2, None)
            }
            _ => unreachable!(),
        };
        match started {
            Ok(mut connection) => {
                let before = connection
                    .config_options()
                    .iter()
                    .filter(|option| option["id"] == "model")
                    .cloned()
                    .collect::<Vec<_>>();
                let variable = format!(
                    "CREWVELA_REAL_{}_MODEL",
                    runtime.as_str().to_ascii_uppercase()
                );
                let selected_model = std::env::var(variable)
                    .ok()
                    .filter(|value| !value.trim().is_empty());
                if let Some(model) = &selected_model {
                    // The production setter rejects values outside the native
                    // advertised catalogue. This changes only this session.
                    connection.set_config_option("model", model)?;
                }
                let model_selection = json!({
                    "runtime": runtime.as_str(), "before": before, "selectedForAcceptance": selected_model,
                    "after": connection.config_options().iter().filter(|option|option["id"] == "model").collect::<Vec<_>>(),
                });
                save(
                    &root,
                    &format!("models-{}.json", runtime.as_str()),
                    &model_selection,
                )?;
                let tools = Arc::new(Mutex::new(Vec::<Value>::new()));
                let observed = tools.clone();
                connection.set_update_handler(Box::new(move |update| {
                    if matches!(update["sessionUpdate"].as_str(),Some("tool_call"|"tool_call_update")) {
                        observed.lock().unwrap().push(json!({
                            "toolCallId":update["toolCallId"],"title":update["title"],"status":update["status"],
                            "isThreadRead":is_thread_read_call(update),
                        }));
                    }
                }));
                eprintln!(
                    "REAL_RUNTIME_MATRIX: {} connected; native session {}",
                    runtime.as_str(),
                    connection.session_id()
                );
                participants.push(ModelParticipant {
                    runtime: *runtime,
                    agent_id: agent_id.clone(),
                    task_id: task_id.clone(),
                    native_session_id: connection.session_id().to_string(),
                    model_selection,
                    connection,
                    tools,
                });
            }
            Err(error) => {
                eprintln!(
                    "REAL_RUNTIME_MATRIX: {} startup failed: {error:#}",
                    runtime.as_str()
                );
                failures.push(json!({"runtime":runtime.as_str(),"stage":"startup","error":format!("{error:#}")}));
            }
        }
    }
    save(
        &root,
        "startup.json",
        &json!({"requested":runtimes.iter().map(|kind|kind.as_str()).collect::<Vec<_>>(),
        "connected":participants.iter().map(|p|p.runtime.as_str()).collect::<Vec<_>>(),"failures":failures}),
    )?;
    let outcome = (|| -> Result<Value> {
        ensure!(
            participants.len() >= 2,
            "fewer than two real Runtime connections available; inspect startup.json"
        );
        let peers = participants
            .iter()
            .map(|p| (p.agent_id.clone(), p.runtime.as_str()))
            .collect::<Vec<_>>();
        let probe = format!("CREWVELA_MATRIX_{}", uuid::Uuid::new_v4().simple());
        let mut expected = Vec::<Value>::new();
        for participant in &mut participants {
            let outbound=peers.iter().filter(|(id,_)|id!=&participant.agent_id).map(|(id,runtime)|{
                let marker=format!("{probe}_{}_TO_{runtime}",participant.runtime.as_str());
                expected.push(json!({"from":participant.agent_id,"to":id,"body":marker,
                    "fromRuntime":participant.runtime.as_str(),"toRuntime":runtime,"task":participant.task_id}));
                json!({"targetAgentId":id,"body":marker})
            }).collect::<Vec<_>>();
            let prompt=format!("Isolated communication acceptance. Use only crewvela MCP cc_message_send. Send exactly each object in this JSON array as a separate tool call: {}. Do not read/write files, use shell, memory, web, or any other tool. After all tool calls succeed reply SENT.",serde_json::to_string(&outbound)?);
            let result = participant
                .connection
                .prompt_interactive(&prompt, &cancelled)
                .with_context(|| {
                    format!("{} real send prompt failed", participant.runtime.as_str())
                })?;
            ensure!(!result.cancelled, "send turn was cancelled");
            ensure!(
                participant.connection.session_id() == participant.native_session_id,
                "native session changed during send"
            );
            let snapshot = store.snapshot(&project.id)?;
            for message in expected
                .iter()
                .filter(|m| m["from"] == participant.agent_id)
            {
                ensure!(
                    snapshot
                        .messages
                        .iter()
                        .any(
                            |actual| actual.sender_id.as_deref() == message["from"].as_str()
                                && actual.target_agent_id.as_deref() == message["to"].as_str()
                                && actual.body == message["body"].as_str().unwrap()
                                && actual.task_id.as_deref() == message["task"].as_str()
                        ),
                    "{} did not persist an exact scoped message",
                    participant.runtime.as_str()
                );
            }
            eprintln!(
                "REAL_RUNTIME_MATRIX: {} persisted {} outbound messages",
                participant.runtime.as_str(),
                outbound.len()
            );
        }
        save(
            &root,
            "outbound.json",
            &json!({"expected":expected,"passed":true}),
        )?;
        for participant in &mut participants {
            let before = participant.tools.lock().unwrap().len();
            let prompt=format!("Isolated communication acceptance. Use only crewvela MCP cc_thread_read and cc_message_send. Read your thread. For every received body starting {probe}_ (exclude messages you sent), send to that message's original senderId a body consisting of ACK: immediately followed by the exact received body. Do not invent received values. Reply RECEIVED only after all acknowledgements succeed. Do not use shell, files, memory, web, or other tools.");
            let result = participant
                .connection
                .prompt_interactive(&prompt, &cancelled)
                .with_context(|| {
                    format!(
                        "{} real receive/reply prompt failed",
                        participant.runtime.as_str()
                    )
                })?;
            ensure!(!result.cancelled, "receive turn was cancelled");
            ensure!(
                participant.connection.session_id() == participant.native_session_id,
                "native session changed during reply"
            );
            let observed = participant.tools.lock().unwrap();
            ensure!(
                read_completed(&observed[before..]),
                "{} must complete a real thread-read tool call: {:?}",
                participant.runtime.as_str(),
                &observed[before..]
            );
            let snapshot = store.snapshot(&project.id)?;
            for original in expected.iter().filter(|m| m["to"] == participant.agent_id) {
                let body = format!("ACK:{}", original["body"].as_str().unwrap());
                ensure!(
                    snapshot
                        .messages
                        .iter()
                        .any(|actual| actual.sender_id.as_deref()
                            == Some(participant.agent_id.as_str())
                            && actual.target_agent_id.as_deref() == original["from"].as_str()
                            && actual.body == body
                            && actual.task_id.as_deref() == Some(participant.task_id.as_str())),
                    "{} did not persist an exact acknowledgement",
                    participant.runtime.as_str()
                );
            }
            eprintln!(
                "REAL_RUNTIME_MATRIX: {} read and acknowledged all peers",
                participant.runtime.as_str()
            );
        }
        for participant in &mut participants {
            let before = participant.tools.lock().unwrap().len();
            let result=participant.connection.prompt_interactive(&format!("Use only crewvela MCP cc_thread_read. Read your received acknowledgements starting ACK:{probe}_, then reply with those exact bodies, one per line. Do not send messages or use any other tools."),&cancelled)?;
            ensure!(!result.cancelled, "receipt-read turn cancelled");
            let observed = participant.tools.lock().unwrap();
            ensure!(
                read_completed(&observed[before..]),
                "{} did not complete receipt read",
                participant.runtime.as_str()
            );
            for original in expected
                .iter()
                .filter(|m| m["from"] == participant.agent_id)
            {
                ensure!(
                    result
                        .text
                        .contains(&format!("ACK:{}", original["body"].as_str().unwrap())),
                    "{} did not return an actual peer receipt",
                    participant.runtime.as_str()
                );
            }
            ensure!(
                participant.connection.session_id() == participant.native_session_id,
                "receipt read changed native session"
            );
        }
        let snapshot = store.snapshot(&project.id)?;
        ensure!(
            snapshot
                .messages
                .iter()
                .filter(|m| m.sender_type == "agent")
                .count()
                == expected.len() * 2,
            "extra message loop or missing acknowledgement"
        );
        Ok(
            json!({"passed":failures.is_empty(),"testedRuntimes":participants.iter().map(|p|p.runtime.as_str()).collect::<Vec<_>>(),
            "directedEdges":expected.len(),"agentMessages":expected.len()*2,"modelPrompts":participants.len()*3,
            "nativeSessionsStable":true,"allMessageBodiesExact":true,"failures":failures,
            "participants":participants.iter().map(|p|json!({"runtime":p.runtime.as_str(),"agentId":p.agent_id,
                "nativeSessionId":p.native_session_id,"modelSelection":p.model_selection,"tools":*p.tools.lock().unwrap()})).collect::<Vec<_>>() }),
        )
    })();
    let result = match &outcome {
        Ok(value) => value.clone(),
        Err(error) => {
            json!({"passed":false,"error":format!("{error:#}"),"startupFailures":failures,
            "participants":participants.iter().map(|p|json!({"runtime":p.runtime.as_str(),"modelSelection":p.model_selection,"tools":*p.tools.lock().unwrap()})).collect::<Vec<_>>() })
        }
    };
    save(&root, "result.json", &result)?;
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../output");
    std::fs::create_dir_all(&output)?;
    let filename = format!("runtime-communication-acp-{}.json", uuid::Uuid::new_v4());
    save(&output, &filename, &result)?;
    eprintln!(
        "REAL_RUNTIME_MATRIX_RESULT={}",
        output.join(filename).display()
    );
    drop(participants);
    drop(leases);
    outcome?;
    ensure!(
        failures.is_empty(),
        "one or more installed Runtime startup checks failed; inspect result.json"
    );
    Ok(())
}
