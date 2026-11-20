//! Explicitly opted-in protocol acceptance. No Tauri app, existing session, or production store.
use super::*;
use crate::acp::{execute_managed_acp, ManagedAcpKind, ManagedAcpMode, ManagedAcpOutcome, UnverifiedStop};

struct RealAcpReplies {
    root: std::path::PathBuf,
    calls: Mutex<Vec<Value>>,
    tools: Arc<Mutex<Vec<Value>>>,
}

impl DiscussionExecutor for RealAcpReplies {
    fn execute(&self, store: &MultiAgentStore, task: &str, run: &str, agent: &str, cancelled: &AtomicBool) -> Result<()> {
        let member = store.agent_profile(agent)?;
        let kind = match member.runtime_kind {
            AgentRuntimeKind::ClaudeCode => ManagedAcpKind::ClaudeCode,
            AgentRuntimeKind::Codex => ManagedAcpKind::Codex,
            _ => bail!("probe only permits Claude and Codex"),
        };
        let prompt = store.snapshot(&member.project_id)?.tasks.into_iter().find(|item| item.id == task).unwrap().description;
        let call_index = {
            let mut calls = self.calls.lock().unwrap();
            if calls.len() >= 5 { bail!("real model probe reached the five-call budget"); }
            let index = calls.len();
            calls.push(json!({"agentId":agent,"runtime":member.runtime_kind,"runId":run,"prompt":prompt,"status":"starting"}));
            index
        };
        let cwd = self.root.join(format!("turn-{call_index}"));
        std::fs::create_dir_all(&cwd)?;
        store.update_run(UpdateRunInput { run_id: run.into(), status: RunStatus::Running,
            output_summary: None, error: None, session_locator: None })?;
        let local_cancel = Arc::new(AtomicBool::new(false));
        let done = AtomicBool::new(false);
        let permission_cancel = local_cancel.clone();
        let permission_tools = self.tools.clone();
        let event_cancel = local_cancel.clone();
        let event_tools = self.tools.clone();
        let event_store = store.clone();
        let event_run = run.to_string();
        // Same managed ACP boundary used by the production discussion executor:
        // fresh session, strict read-only mode, no collaboration/user MCP injection,
        // no model override. Deny every permission; any observed tool call fails.
        let outcome = std::thread::scope(|scope| {
            scope.spawn(|| {
                let started = Instant::now();
                while !done.load(Ordering::Acquire) {
                    if cancelled.load(Ordering::Acquire) || started.elapsed() >= Duration::from_secs(120) {
                        local_cancel.store(true, Ordering::Release);
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            });
            let outcome = execute_managed_acp(kind, &format!("isolated-chat-probe-{}", id()), &cwd.display().to_string(),
                None, &prompt, "", "", ManagedAcpMode::ReadOnly, &local_cancel, None,
                Some(Box::new(move |_, _| {
                    permission_tools.lock().unwrap().push(json!({"event":"permission_request","denied":true}));
                    permission_cancel.store(true, Ordering::Release);
                    Ok(false)
                })),
                Some(Box::new(move |update| {
                    let kind = update.get("sessionUpdate").and_then(Value::as_str);
                    if matches!(kind, Some("tool_call" | "tool_call_update")) {
                        event_tools.lock().unwrap().push(json!({"event":kind,"toolCallId":update.get("toolCallId"),"title":update.get("title")}));
                        event_cancel.store(true, Ordering::Release);
                    } else if kind == Some("agent_message_chunk") {
                        if let Some(text) = update.pointer("/content/text").and_then(Value::as_str) {
                            if event_store.append_runtime_event(super::super::super::AppendRuntimeEventInput {
                                run_id: event_run.clone(), event_type: "assistant_delta".into(), payload: json!({"text":text}),
                            }).is_err() { event_cancel.store(true, Ordering::Release); }
                        }
                    }
                })), None, &[], None);
            done.store(true, Ordering::Release);
            outcome
        });
        match outcome {
            Ok(ManagedAcpOutcome::Completed(result)) => {
                self.calls.lock().unwrap()[call_index]["status"] = json!("completed");
                self.calls.lock().unwrap()[call_index]["output"] = json!(result.text);
                self.calls.lock().unwrap()[call_index]["protocolSessionId"] = json!(result.session_id);
                self.calls.lock().unwrap()[call_index]["agentVersion"] = json!(result.agent_version);
                if !self.tools.lock().unwrap().is_empty() { bail!("the chat probe attempted a tool; no further models will run"); }
                store.finalize_successful_run(run, "isolated protocol acceptance", &result.text, false,
                    None, Some(&json!({"adapter":"acp","cwd":cwd,"sessionId":result.session_id})))
            }
            other => {
                let error = match other { Ok(ManagedAcpOutcome::Unavailable(reason)) => anyhow!(reason), Err(error) => error, _ => unreachable!() };
                self.calls.lock().unwrap()[call_index]["status"] = json!("failed");
                self.calls.lock().unwrap()[call_index]["error"] = json!(format!("{error:#}"));
                if error.downcast_ref::<UnverifiedStop>().is_some() { store.mark_run_stop_unverified(run, &error.to_string())?; }
                Err(error)
            }
        }
    }
}

#[test]
#[ignore = "requires CREWVELA_RUN_REAL_CHAT=1, local logged-in Claude/Codex and network; at most five new model calls"]
fn real_claude_codex_chat_and_exchange_use_current_local_defaults() {
    assert_eq!(std::env::var("CREWVELA_RUN_REAL_CHAT").as_deref(), Ok("1"), "explicit opt-in required");
    let base = std::env::var_os("CREWVELA_CHAT_PROBE_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let fixture = Fixture::at(base.join(format!("crewvela-real-chat-{}", id())));
    eprintln!("REAL_CHAT_PROBE_DIR={}", fixture.root.display());
    {
        let connection = fixture.store.connection.lock().unwrap();
        connection.execute("UPDATE ma_projects SET name='隔离群聊验收',goal='普通聊天与两位成员自然交流，不涉及工程执行' WHERE id=?1", [&fixture.project]).unwrap();
        for (index, name, runtime) in [(0, "Claude", AgentRuntimeKind::ClaudeCode), (1, "Codex", AgentRuntimeKind::Codex)] {
            connection.execute("UPDATE ma_agents SET name=?2,runtime_kind=?3,description=?4 WHERE id=?1", params![fixture.agents[index], name, runtime.as_str(),
                "用简短中文自然交流。简单问候时各自礼貌回应一句；不调用任何工具，不读写文件，不访问网络，不向任何会话发消息。用户要求交流时只围绕公开前文接话。"]).unwrap();
        }
    }
    let executor = RealAcpReplies { root: fixture.root.clone(), calls: Mutex::new(Vec::new()), tools: Arc::new(Mutex::new(Vec::new())) };
    let mut checks = json!({"chat":false,"exchange":false});
    let outcome = (|| -> Result<()> {
        let mut input = fixture.input();
        input.body = "hi".into();
        input.max_duration_seconds = 300;
        let (round, _) = fixture.store.begin_conversation(input)?;
        run_discussion(&fixture.store, &round.id, &Arc::new(AtomicBool::new(false)), &executor)?;
        let chat = fixture.store.conversation_snapshot(&fixture.project)?;
        std::fs::write(fixture.root.join("chat-snapshot.json"), serde_json::to_vec_pretty(&chat)?)?;
        if chat.rounds[0].status != "completed" { bail!("real chat did not complete: {:?}", chat.rounds[0].stop_reason); }
        if executor.calls.lock().unwrap().len() != 2 || chat.messages.iter().filter(|message| message.sender_type == "agent").count() != 2 { bail!("hi must call and receive one reply from each selected member"); }
        checks["chat"] = json!(true);
        let mut input = fixture.input();
        input.client_message_id = "real-exchange".into();
        input.intent = "exchange".into();
        input.max_duration_seconds = 300;
        input.body = "@Claude @Codex 请交流一个普通待办页的输入校验。Claude 先提一个具体建议并 @Codex 问一个边界；Codex 要基于 Claude 的真实建议补充一个具体边界，然后结束交流，不再交接。每位一句话，不调用工具。".into();
        let (round, _) = fixture.store.begin_conversation(input)?;
        run_discussion(&fixture.store, &round.id, &Arc::new(AtomicBool::new(false)), &executor)?;
        let snapshot = fixture.store.conversation_snapshot(&fixture.project)?;
        let exchange = snapshot.rounds.iter().find(|item| item.id == round.id).unwrap();
        if exchange.status != "completed" { bail!("real exchange did not complete: {:?}", exchange.stop_reason); }
        let replies = snapshot.messages.iter().filter(|message| message.round_id.as_deref() == Some(round.id.as_str()) && message.sender_type == "agent").collect::<Vec<_>>();
        if replies.len() != 2 || replies[1].reply_to_message_id.as_ref() != Some(&replies[0].id) { bail!("exchange did not make exactly two directed public replies"); }
        let calls = executor.calls.lock().unwrap();
        if calls.len() != 4 || !calls[3]["prompt"].as_str().unwrap_or_default().contains(&replies[0].body) { bail!("second member did not receive the first member's real public response"); }
        if !executor.tools.lock().unwrap().is_empty() { bail!("tool activity was observed"); }
        checks["exchange"] = json!(true);
        Ok(())
    })();
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    std::fs::write(fixture.root.join("conversation-snapshot.json"), serde_json::to_vec_pretty(&snapshot).unwrap()).unwrap();
    let evidence = json!({"passed":outcome.is_ok(),"checks":checks,"error":outcome.as_ref().err().map(|error| format!("{error:#}")),
        "boundary":"production run_discussion + production managed ACP protocol; isolated store; not native Tauri GUI E2E",
        "storePath":fixture.root.join("store.db"),"modelSelection":"unchanged local CLI defaults",
        "calls":*executor.calls.lock().unwrap(),"toolEvents":*executor.tools.lock().unwrap()});
    std::fs::write(fixture.root.join("acceptance.json"), serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    eprintln!("REAL_CHAT_ACCEPTANCE={} evidence={}", outcome.is_ok(), fixture.root.join("acceptance.json").display());
    assert!(outcome.is_ok(), "{}", outcome.unwrap_err());
}
