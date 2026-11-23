use super::*;

#[test]
fn fresh_conversation_reports_native_cli_engines() {
    let fixture=Fixture::new();
    let snapshot=fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.supported_discussion_runtimes.len(),8);
    assert!(snapshot.supported_discussion_runtimes.contains(&AgentRuntimeKind::Antigravity));
    assert!(!snapshot.supported_discussion_runtimes.contains(&AgentRuntimeKind::ZCode));
    assert!(snapshot.supported_discussion_runtimes.contains(&AgentRuntimeKind::DeepSeekHarness));
}

#[test]
fn antigravity_accepts_only_native_discussion() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    let runtime = AgentRuntimeKind::Antigravity;
    assert!(validate_discussion_only_member(runtime, &input).is_err());
    input.native_capabilities = true;
    assert!(validate_discussion_only_member(runtime, &input).is_ok());
    input.mode = "execution_plan".into();
    assert!(validate_discussion_only_member(runtime, &input).is_err());
    assert!(validate_discussion_only_member(AgentRuntimeKind::Codex, &input).is_ok());
    input.mode = "discussion".into();
    input.task_rules = Some(Default::default());
    assert!(validate_discussion_only_member(runtime, &input).is_err());
}

#[test]
fn existing_desktop_member_keeps_native_group_compatibility() {
    let fixture=Fixture::new();
    fixture.store.connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind='zcode',model='desktop/original' WHERE id=?1",[&fixture.agents[0]]).unwrap();
    let snapshot=fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert_eq!(snapshot.supported_discussion_runtimes.len(),9);
    assert!(snapshot.supported_discussion_runtimes.contains(&AgentRuntimeKind::ZCode));
    let mut input=fixture.input();input.native_capabilities=true;input.participant_agent_ids=vec![fixture.agents[0].clone()];
    let (round,_)=fixture.store.begin_conversation(input).unwrap();
    assert!(round.member_policies[&fixture.agents[0]].uses_native_capabilities());
    let member=fixture.store.agent_profile(&fixture.agents[0]).unwrap();assert_eq!(member.runtime_kind,AgentRuntimeKind::ZCode);assert_eq!(member.model.as_deref(),Some("desktop/original"));
}

#[test]
fn native_group_accepts_every_active_cli_and_role_does_not_gate_capabilities() {
    for runtime in discussion_runtimes() {
        let fixture = Fixture::new();
        let agent = &fixture.agents[0];
        fixture.store.connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind=?2,description='模型验收：只回验收标记、不调用工具',permission_profile='default' WHERE id=?1", params![agent,runtime.as_str()]).unwrap();
        let mut input = fixture.input(); input.native_capabilities = true; input.participant_agent_ids = vec![agent.clone()];
        input.body = "读代码并分析问题，只分析不修改".into();
        let (round, _) = fixture.store.begin_conversation(input).unwrap();
        assert!(round.member_policies[agent].uses_native_capabilities());
        let turn = prepare_turn(&fixture.store, &round.id, 0, "chat", 0, agent).unwrap();
        let task = fixture.store.snapshot(&fixture.project).unwrap().tasks.into_iter().find(|task| task.id == turn.task_id).unwrap();
        assert_eq!(task.workspace_mode, WorkspaceMode::SharedWrite);
        assert!(task.description.contains("保留当前 CLI 自带的工具、联网、Skill、MCP"));
        assert!(!task.description.contains("这是只读会话，禁止"));
        assert!(!task.description.contains("本次为纯回复"));
        assert!(task.description.contains("只分析不修改"));
        assert_eq!(fixture.store.agent_profile(agent).unwrap().permission_profile, PermissionProfile::Default);
    }
}

#[test]
fn native_request_is_frozen_and_old_requests_keep_their_original_contract() {
    let fixture = Fixture::new();
    let mut input = fixture.input(); input.native_capabilities = true;
    let (round, _) = fixture.store.begin_conversation(input.clone()).unwrap();
    input.native_capabilities = false;
    assert!(fixture.store.begin_conversation(input).is_err(), "cannot change capability semantics under the same message id");
    let mut old = serde_json::to_value(round).unwrap(); old.as_object_mut().unwrap().remove("memberPolicies");
    let old: ConversationRound = serde_json::from_value(old).unwrap();
    assert!(old.member_policies.is_empty());
    assert!(!EffectiveConversationPolicy::legacy().uses_native_capabilities());
}

#[test]
#[ignore = "requires CREWVELA_RUN_REAL_CODEX_MODEL=1 and isolated CODEX_HOME; one short request through the production group executor"]
fn real_native_codex_group_inherits_model_and_completes() {
    assert_eq!(std::env::var("CREWVELA_RUN_REAL_CODEX_MODEL").as_deref(), Ok("1"));
    let base = std::path::PathBuf::from(std::env::var_os("CREWVELA_CODEX_MODEL_PROBE_ROOT").expect("isolated root required"));
    let code_home = std::path::PathBuf::from(std::env::var_os("CODEX_HOME").expect("isolated CODEX_HOME required"));
    assert_eq!(code_home.canonicalize().unwrap(), base.join("codex-home").canonicalize().unwrap());
    assert!(base.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
    assert_eq!(std::fs::read_to_string(base.join(".isolated-model-probe")).unwrap(), "crewvela-codex-model-probe-v1\n");
    crate::test_profile::initialize(&base.join("app")).unwrap();
    let fixture = Fixture::at(base.join(format!("group-{}", id())));
    fixture.store.connection.lock().unwrap().execute(
        "UPDATE ma_agents SET model=NULL,permission_profile='default',description='隔离普通群聊验收，不调用工具' WHERE id=?1",
        [&fixture.agents[0]],
    ).unwrap();
    let sessions = crate::sessions::SessionManager::open_at(fixture.root.join("sessions.json"), fixture.root.join("events")).unwrap();
    let executor = ExistingRunExecutor {
        host: AgentTerminalHost::new(crate::pty::PtyManager::new(), sessions),
        bridge: CollaborationBridge::start(fixture.store.clone()).unwrap(),
    };
    let marker = "CREWVELA_CODEX_GROUP_MODEL_OK";
    let mut input = fixture.input();
    input.native_capabilities = true;
    input.participant_agent_ids = vec![fixture.agents[0].clone()];
    input.max_duration_seconds = 90;
    input.body = format!("只回复 {marker}，不要调用工具、读取文件、发信或执行其他任务。");
    let (round, _) = fixture.store.begin_conversation(input).unwrap();
    let outcome = run_discussion(&fixture.store, &round.id, &Arc::new(AtomicBool::new(false)), &executor);
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    let engine = fixture.store.snapshot(&fixture.project).unwrap();
    let events = fixture.store.runtime_events(&fixture.project, None, 2000).unwrap();
    let round = snapshot.rounds.iter().find(|item| item.id == round.id).unwrap();
    let replies = snapshot.messages.iter().filter(|message| message.sender_type == "agent").collect::<Vec<_>>();
    let tool_seen = events.iter().any(|event| event.event_type == "tool_protocol");
    let native_history = (|| -> anyhow::Result<bool> {
        let run = engine.runs.iter().find(|run| run.agent_id == fixture.agents[0])
            .context("团队聊天未启动 Codex 成员")?;
        ensure!(run.session_locator.as_ref().is_some_and(|locator| locator["codexStorage"] == "crewvela"),
            "团队聊天缺少隔离会话归属");
        let reopened = MultiAgentStore::open_at(fixture.root.join("store.db"))?;
        let source = reopened.conversation_run_locator(&fixture.project, &run.id)?;
        let history = crate::session_switch_history::read_conversation_run(&source)?;
        Ok(history.iter().any(|message| message.role == "assistant" && message.text.contains(marker)))
    })();
    let evidence = json!({"boundary":"production run_discussion -> ExistingRunExecutor -> execute_preferred_acp; isolated store/profile/CODEX_HOME; no Tauri GUI",
        "passed":outcome.is_ok() && round.status == "completed" && replies.len() == 1 && replies[0].body.trim() == marker && !tool_seen && native_history.as_ref().is_ok_and(|matched| *matched),
        "roundStatus":round.status,"stopReason":round.stop_reason,"toolSeen":tool_seen,
        "nativeReadback":native_history.as_ref().ok(),"nativeReadbackError":native_history.as_ref().err().map(|error|format!("{error:#}")),
        "memberModel":fixture.store.agent_profile(&fixture.agents[0]).unwrap().model,
        "replies":replies.iter().map(|message|json!({"status":message.status,"body":message.body})).collect::<Vec<_>>(),
        "runs":engine.runs.iter().map(|run|json!({"status":run.status,"sessionLocator":run.session_locator})).collect::<Vec<_>>(),
        "runtimeEventTypes":events.iter().map(|event|event.event_type.as_str()).collect::<Vec<_>>(),
        "error":outcome.as_ref().err().map(|error|format!("{error:#}"))});
    let path = base.join("group-acceptance.json");
    std::fs::write(&path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
    eprintln!("CODEX_GROUP_MODEL_ACCEPTANCE={} evidence={}", evidence["passed"], path.display());
    outcome.unwrap();
    assert_eq!(evidence["passed"], true, "group completion must be stored with the real Codex response");
}

#[test]
#[ignore = "requires CREWVELA_RUN_NATIVE_GROUP=1 and logged-in native CLIs; isolated project and store"]
fn real_native_group_reads_with_original_cli_tools_and_keeps_role() {
    assert_eq!(std::env::var("CREWVELA_RUN_NATIVE_GROUP").as_deref(), Ok("1"));
    let base = std::env::var_os("CREWVELA_NATIVE_PROBE_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let runtimes = std::env::var("CREWVELA_NATIVE_PROBE_RUNTIMES").unwrap_or_else(|_| "codex,claude_code,zcode_cli".into());
    for runtime in runtimes.split(',') {
        let runtime = match runtime { "codex" => AgentRuntimeKind::Codex, "claude_code" => AgentRuntimeKind::ClaudeCode, "zcode_cli" => AgentRuntimeKind::ZCodeCli, "pi" => AgentRuntimeKind::Pi, _ => panic!("unknown native probe runtime") };
        let fixture = Fixture::at(base.join(format!("native-{}-{}", runtime.as_str(), id())));
        let marker = format!("NATIVE_READ_{}", id());
        std::fs::write(fixture.root.join("project/probe.txt"), &marker).unwrap();
        fixture.store.connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind=?2,description='擅长模型设置验收；历史验收只回标记、不调用工具',permission_profile='default' WHERE id=?1", params![fixture.agents[0], runtime.as_str()]).unwrap();
        let sessions = crate::sessions::SessionManager::open_at(fixture.root.join("sessions.json"), fixture.root.join("events")).unwrap();
        let executor = ExistingRunExecutor { host:AgentTerminalHost::new(crate::pty::PtyManager::new(), sessions), bridge:CollaborationBridge::start(fixture.store.clone()).unwrap() };
        let done = Arc::new(AtomicBool::new(false)); let watch_done = done.clone(); let watch_store = fixture.store.clone();
        let watcher = std::thread::spawn(move || {
            let governance = crate::multi_agent::governance::CollaborationGovernance::new(watch_store);
            while !watch_done.load(Ordering::Acquire) {
                for item in governance.list_interventions().unwrap_or_default() {
                    if item.kind == crate::multi_agent::InterventionKind::ApprovalRequired {
                        // Simulate the human approving THIS isolated read-only test request.
                        governance.resolve(crate::multi_agent::ResolveInterventionInput { intervention_id:item.id,action:crate::multi_agent::InterventionAction::Approve,response:None,agent_id:None,snooze_minutes:None }).unwrap();
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        });
        let mut input = fixture.input(); input.native_capabilities = true; input.participant_agent_ids = vec![fixture.agents[0].clone()]; input.max_duration_seconds = 180;
        input.body = "本次是普通群聊中的文件读取请求，不是模型验收。请使用你当前 CLI 原有的文件读取工具读取项目根目录的 probe.txt，准确返回其中的随机字符串；不要猜测，不修改任何文件，不发信，不运行其他任务。".into();
        let outcome = fixture.store.begin_conversation(input).and_then(|(round,_)| run_discussion(&fixture.store, &round.id, &Arc::new(AtomicBool::new(false)), &executor));
        done.store(true, Ordering::Release); watcher.join().unwrap();
        let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
        let events = fixture.store.runtime_events(&fixture.project,None,2000).unwrap();
        let read = snapshot.messages.iter().any(|m| m.sender_type == "agent" && m.body.contains(&marker));
        let evidence = json!({"runtime":runtime,"nativeRead":read,"error":outcome.as_ref().err().map(ToString::to_string),"snapshot":snapshot,"runtimeEvents":events});
        let path=fixture.root.join("evidence.json"); std::fs::write(&path,serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
        eprintln!("NATIVE_GROUP_EVIDENCE={} read={read}",path.display());
        outcome.unwrap();
        assert!(read,"native CLI must read the unknown marker, not just claim tools are available");
        assert_eq!(std::fs::read_to_string(fixture.root.join("project/probe.txt")).unwrap(),marker);
    }
}

#[test]
#[ignore = "requires CREWVELA_RUN_WEB_PERMISSION=1 and logged-in Claude; isolated projects only"]
fn real_native_claude_web_permission_preserves_explicit_ask_and_deny() {
    assert_eq!(std::env::var("CREWVELA_RUN_WEB_PERMISSION").as_deref(), Ok("1"));
    let base = std::env::var_os("CREWVELA_WEB_PROBE_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let cases = std::env::var("CREWVELA_WEB_PROBE_CASES").unwrap_or_else(|_| "default,ask,deny".into());
    for case in cases.split(',') {
        assert!(["default","ask","deny"].contains(&case));
        let fixture = Fixture::at(base.join(format!("web-{case}-{}",id())));
        let config = fixture.root.join("project/.claude/settings.local.json");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        let settings = if case=="default" { json!({}) } else { json!({"permissions":{case:["WebSearch"]}}) };
        let original = serde_json::to_vec_pretty(&settings).unwrap();
        std::fs::write(&config,&original).unwrap();
        fixture.store.connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind='claude_code',permission_profile='default' WHERE id=?1", [&fixture.agents[0]]).unwrap();
        let sessions=crate::sessions::SessionManager::open_at(fixture.root.join("sessions.json"),fixture.root.join("events")).unwrap();
        let executor=ExistingRunExecutor {host:AgentTerminalHost::new(crate::pty::PtyManager::new(),sessions),bridge:CollaborationBridge::start(fixture.store.clone()).unwrap()};
        let cancelled=Arc::new(AtomicBool::new(false));let worker_cancel=cancelled.clone();
        let done=Arc::new(AtomicBool::new(false));let watch_done=done.clone();let watch_store=fixture.store.clone();
        let watch_case=case.to_string();
        let watcher=std::thread::spawn(move|| {
            let governance=crate::multi_agent::governance::CollaborationGovernance::new(watch_store);
            let mut approvals=HashSet::new();
            while !watch_done.load(Ordering::Acquire) {
                for item in governance.list_interventions().unwrap() {
                    if item.kind!=crate::multi_agent::InterventionKind::ApprovalRequired {continue;}
                    approvals.insert(item.id.clone());
                    if watch_case=="ask" {
                        let receipt=governance.resolve(crate::multi_agent::ResolveInterventionInput {intervention_id:item.id,action:crate::multi_agent::InterventionAction::Approve,response:None,agent_id:None,snooze_minutes:None}).unwrap();
                        assert!(!receipt.resume_project);
                    } else {worker_cancel.store(true,Ordering::Release);}
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            approvals.len()
        });
        let mut input=fixture.input();input.native_capabilities=true;input.participant_agent_ids=vec![fixture.agents[0].clone()];input.max_duration_seconds=90;
        input.body="这是隔离的联网授权检查。必须尝试一次原生 WebSearch，查询 OpenAI Codex official documentation。只使用 WebSearch，不得用 Bash、WebFetch 或其他工具替代；不修改文件、不发消息、不启动工程。若原生工具被禁止就简短说明。不要根据记忆伪造搜索结果。".into();
        let outcome=fixture.store.begin_conversation(input).and_then(|(round,_)|run_discussion(&fixture.store,&round.id,&cancelled,&executor));
        done.store(true,Ordering::Release);let approvals=watcher.join().unwrap();
        outcome.unwrap();
        let snapshot=fixture.store.snapshot(&fixture.project).unwrap();
        let tools=snapshot.runtime_events.iter().filter(|e|e.event_type=="tool_protocol"&&e.payload.pointer("/_meta/claudeCode/toolName").and_then(Value::as_str)==Some("WebSearch")).collect::<Vec<_>>();
        if case=="ask" {assert!(approvals>0,"explicit ask must not be bypassed");} else {assert_eq!(approvals,0,"{case}: unexpected human approval");}
        let excluded=snapshot.runtime_events.iter().any(|e|e.event_type=="tool_protocol"
            && e.payload.pointer("/_meta/claudeCode/toolName").and_then(Value::as_str)==Some("ToolSearch")
            && e.payload.pointer("/_meta/claudeCode/toolResponse/query").and_then(Value::as_str)==Some("select:WebSearch")
            && e.payload.pointer("/_meta/claudeCode/toolResponse/matches")==Some(&json!([])));
        let denied=tools.iter().any(|e|e.payload["status"]=="failed") || excluded;
        if case=="deny" {
            // Native deny can remove the deferred tool from the catalog before
            // invocation. Assert real ToolSearch evidence, never a model claim.
            assert!(denied,"explicit deny must prevent search");
            assert!(!tools.iter().any(|e|e.payload["status"]=="completed"));
        } else {
            assert!(!tools.is_empty(),"{case}: no actual native WebSearch event");
            assert!(tools.iter().any(|e|e.payload["status"]=="completed"),"{case}: WebSearch did not complete");
        }
        assert_eq!(std::fs::read(&config).unwrap(),original,"single-use authorization must not persist permissions");
        eprintln!("WEB_PERMISSION case={case} native_events={} approvals={approvals} denied={denied}",tools.len());
    }
}

#[test]
fn reply_only_is_an_immutable_request_ceiling_and_does_not_leak_to_followups() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.body = "本次只回复ABC，不调用工具".into();
    let executor = ScriptedReplies::new(vec!["ABC".into(), "ABC".into()]);
    let first = fixture.run_input(input.clone(), &executor);
    assert!(first.rounds[0].member_policies.values().all(|p| !p.allows_tools()));
    let (retried, started) = fixture.store.begin_conversation(input).unwrap();
    assert!(!started);
    assert!(retried.member_policies.values().all(|p| !p.allows_tools()));
    let mut next = fixture.input(); next.client_message_id = "research-followup".into(); next.body = "读取项目代码分析原因".into();
    let (round, _) = fixture.store.begin_conversation(next).unwrap();
    assert!(round.member_policies.values().all(EffectiveConversationPolicy::allows_tools));
    let prompt = discussion_prompt(&fixture.store, &fixture.store.connection.lock().unwrap(), &round, "chat", 0, &fixture.agents[0]).unwrap();
    assert!(prompt.contains("过去的临时验收禁令和执行授权不自动延续"));
    assert!(prompt.contains("本次允许研究"));
    let mut old = serde_json::to_value(&round).unwrap(); old.as_object_mut().unwrap().remove("memberPolicies");
    let old: ConversationRound = serde_json::from_value(old).unwrap();
    let old_prompt = discussion_prompt(&fixture.store, &fixture.store.connection.lock().unwrap(), &old, "chat", 0, &fixture.agents[0]).unwrap();
    assert!(old_prompt.contains("本次为纯回复"));
}

#[test]
fn role_and_no_write_text_do_not_disable_reading_or_web_research() {
    let fixture = Fixture::new();
    {
        let connection = fixture.store.connection.lock().unwrap();
        connection.execute("UPDATE ma_agents SET description='后端成员不修改前端、不提交代码；职责仅负责验收' WHERE id=?1", [&fixture.agents[0]]).unwrap();
        connection.execute("UPDATE ma_agents SET description='负责审查；长期禁止联网' WHERE id=?1", [&fixture.agents[1]]).unwrap();
    }
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    assert_eq!(round.member_policies[&fixture.agents[0]].boundary.tools, "research");
    assert_eq!(round.member_policies[&fixture.agents[0]].boundary.network, "public_web");
    assert_eq!(round.member_policies[&fixture.agents[1]].boundary.tools, "research");
    assert_eq!(round.member_policies[&fixture.agents[1]].boundary.network, "none");
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert!(!snapshot.member_policies.iter().find(|p| p.agent_id.as_ref() == Some(&fixture.agents[0])).unwrap().legacy_review_required);
    assert!(snapshot.member_policies.iter().find(|p| p.agent_id.as_ref() == Some(&fixture.agents[1])).unwrap().legacy_review_required);
}

#[test]
fn explicit_legacy_no_tools_survives_until_reviewed_migration_and_archives_exact_original() {
    let fixture = Fixture::new();
    let original = "仅执行指定模型验收；不调用工具、不修改文件、只回复验收标记";
    fixture.store.connection.lock().unwrap().execute("UPDATE ma_agents SET description=?2 WHERE id=?1", params![fixture.agents[0], original]).unwrap();
    let preview = fixture.store.conversation_migration_preview(ConversationMigrationPreviewInput { project_id: fixture.project.clone(), agent_id: Some(fixture.agents[0].clone()), replacement_text: "负责模型验收、证据核对；不修改前端".into() }).unwrap();
    assert_eq!(preview.archived_rules, original);
    assert_eq!(fixture.store.agent_profile(&fixture.agents[0]).unwrap().description, original, "preview must not mutate source");
    let snapshot = fixture.store.conversation_migration_apply(ConversationMigrationApplyInput { project_id: fixture.project.clone(), preview_id: preview.id.clone() }).unwrap();
    assert_eq!(snapshot.archived_task_rules[0].instructions, original);
    assert!(!snapshot.member_policies.iter().find(|p| p.agent_id.as_ref() == Some(&fixture.agents[0])).unwrap().legacy_review_required);
    assert_eq!(fixture.store.conversation_migration_apply(ConversationMigrationApplyInput { project_id: fixture.project.clone(), preview_id: preview.id.clone() }).unwrap().archived_task_rules.len(), 1, "apply must be idempotent");
    let mut input = fixture.input(); input.task_rules = Some(ConversationTaskRules { archived_rule_id: Some(preview.id), ..Default::default() });
    let (round, _) = fixture.store.begin_conversation(input).unwrap();
    assert!(!round.member_policies[&fixture.agents[0]].allows_tools());
    assert!(round.member_policies[&fixture.agents[0]].task_instructions.contains(original));
    assert!(round.member_policies[&fixture.agents[1]].allows_tools(), "one member's archived rule must not constrain another member");
}

#[test]
fn migration_rejects_stale_source_and_does_not_relax_long_term_policy() {
    let fixture = Fixture::new();
    fixture.store.conversation_policy_set(ConversationPolicySetInput { project_id: fixture.project.clone(), agent_id: None, boundary: ConversationHardBoundary { network: "none".into(), ..Default::default() } }).unwrap();
    let preview = fixture.store.conversation_migration_preview(ConversationMigrationPreviewInput { project_id: fixture.project.clone(), agent_id: None, replacement_text: "新的项目目标".into() }).unwrap();
    fixture.store.connection.lock().unwrap().execute("UPDATE ma_projects SET goal='用户在别处修改了目标' WHERE id=?1", [&fixture.project]).unwrap();
    assert!(fixture.store.conversation_migration_apply(ConversationMigrationApplyInput { project_id: fixture.project.clone(), preview_id: preview.id }).unwrap_err().to_string().contains("原规则已变化"));
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert!(snapshot.archived_task_rules.is_empty());
    assert_eq!(snapshot.project_policy.boundary.network, "none");
}

#[test]
fn output_format_and_network_only_archives_do_not_disable_local_research() {
    let fixture = Fixture::new();
    fixture.store.connection.lock().unwrap().execute("UPDATE ma_agents SET description='长期禁止联网，但可以读取项目代码' WHERE id=?1", [&fixture.agents[0]]).unwrap();
    let preview = fixture.store.conversation_migration_preview(ConversationMigrationPreviewInput { project_id:fixture.project.clone(), agent_id:Some(fixture.agents[0].clone()), replacement_text:"负责研究和验收".into() }).unwrap();
    let snapshot = fixture.store.conversation_migration_apply(ConversationMigrationApplyInput {project_id:fixture.project.clone(),preview_id:preview.id.clone()}).unwrap();
    assert_eq!(snapshot.archived_task_rules[0].tools, "research");
    let mut input = fixture.input(); input.body = "搜索最新资料，只回复结论；only reply with the model name".into();
    input.task_rules = Some(ConversationTaskRules {archived_rule_id:Some(preview.id), ..Default::default()});
    let (round,_) = fixture.store.begin_conversation(input).unwrap();
    assert!(round.member_policies.values().all(EffectiveConversationPolicy::allows_tools));
    assert_eq!(round.member_policies[&fixture.agents[0]].boundary.network, "none");
    assert_eq!(round.member_policies[&fixture.agents[1]].boundary.network, "public_web");
}

#[test]
fn current_message_network_and_path_limits_are_enforced_without_contaminating_followups() {
    let fixture = Fixture::new();
    let mut input = fixture.input(); input.body = "本次禁止联网，读本地代码".into();
    let first = fixture.run_input(input, &ScriptedReplies::new(vec!["好的".into(),"好的".into()]));
    assert!(first.rounds[0].member_policies.values().all(|p| p.allows_tools() && p.boundary.network == "none"));
    let mut next = fixture.input(); next.client_message_id = "next-with-network".into();
    let second = fixture.run_input(next, &ScriptedReplies::new(vec!["好的".into(),"好的".into()]));
    assert!(second.rounds.last().unwrap().member_policies.values().all(|p| p.allows_tools() && p.boundary.network == "public_web"));
    let mut restricted = fixture.input(); restricted.client_message_id = "no-private-path".into(); restricted.body = "本次不要读取 private 目录，分析一下".into();
    let (round,_) = fixture.store.begin_conversation(restricted).unwrap();
    assert!(round.member_policies.values().all(|p| !p.allows_tools() && p.restriction_reasons.iter().any(|reason| reason.contains("读取限制"))));
}

#[test]
fn unsupported_codex_conversation_fails_before_starting_a_native_or_less_restricted_session() {
    let fixture = Fixture::new();
    let sessions = crate::sessions::SessionManager::open_at(fixture.root.join("sessions.json"), fixture.root.join("events")).unwrap();
    let executor = ExistingRunExecutor {host:AgentTerminalHost::new(crate::pty::PtyManager::new(),sessions),bridge:CollaborationBridge::start(fixture.store.clone()).unwrap()};
    let mut input = fixture.input(); input.participant_agent_ids = vec![fixture.agents[0].clone()];
    let (round,_) = fixture.store.begin_conversation(input).unwrap();
    run_discussion(&fixture.store,&round.id,&Arc::new(AtomicBool::new(false)),&executor).unwrap();
    let snapshot = fixture.store.conversation_snapshot(&fixture.project).unwrap();
    assert_ne!(snapshot.rounds[0].status,"completed");
    assert!(snapshot.rounds[0].stop_reason.as_deref().unwrap_or("").contains("完整群聊工具白名单"));
    assert!(!snapshot.messages.iter().any(|message|message.sender_type == "agent" && message.status == "completed"));
    let native_count:i64 = fixture.store.connection.lock().unwrap().query_row("SELECT COUNT(*) FROM ma_agent_sessions WHERE project_id=?1 AND app_session_id IS NOT NULL",[&fixture.project],|row|row.get(0)).unwrap();
    assert_eq!(native_count,0,"unsupported policy must not open a fallback native session");
}

fn active_turn(fixture: &Fixture) -> (Turn, String) {
    active_turn_with_locator(fixture, None)
}

fn active_turn_with_locator(fixture: &Fixture, locator: Option<Value>) -> (Turn, String) {
    let (round, _) = fixture.store.begin_conversation(fixture.input()).unwrap();
    let turn = prepare_turn(&fixture.store, &round.id, 0, "chat", 0, &fixture.agents[0]).unwrap();
    let run = fixture.store.claim_task(ClaimTaskInput { task_id: turn.task_id.clone(), agent_id: fixture.agents[0].clone(), lease_seconds:120 }).unwrap();
    fixture.store.update_run(UpdateRunInput { run_id:run.id.clone(), status:RunStatus::Running, output_summary:None,error:None,session_locator:locator }).unwrap();
    (turn, run.id)
}

#[test]
fn managed_file_reads_follow_the_running_worktree_and_current_project_and_member_exclusions() -> Result<()> {
    let fixture = Fixture::new();
    let project_dir = fixture.root.join("project");
    let worktree = fixture.root.join("working");
    std::fs::create_dir(&worktree)?;
    let agent = &fixture.agents[0];
    fixture.store.set_project_status(&fixture.project, ProjectStatus::Active)?;
    let (_, run) = active_turn_with_locator(&fixture, Some(json!({"cwd":worktree})));
    for file in ["allowed.txt", "project.txt", "member.txt"] { std::fs::write(worktree.join(file), file)?; }
    let read = |root: &std::path::Path, name: &str| {
        research::workspace_call(&fixture.store, &run, agent, root, "cc_research_read", &json!({"path":name}))
    };
    assert_eq!(read(&worktree, "project.txt")?["text"], "project.txt");
    for (member, denied) in [(None, project_dir.join("project.txt").display().to_string()), (Some(agent.clone()), "member.txt".into())] {
        fixture.store.conversation_policy_set(ConversationPolicySetInput {
            project_id: fixture.project.clone(), agent_id: member,
            boundary: ConversationHardBoundary { denied_read_paths: vec![denied], ..Default::default() },
        })?;
    }
    assert_eq!(read(&worktree, "allowed.txt")?["text"], "allowed.txt");
    for file in ["project.txt", "member.txt"] { assert!(read(&worktree, file).unwrap_err().to_string().contains("禁止读取")); }
    assert!(read(&project_dir, "allowed.txt").unwrap_err().to_string().contains("工作树与当前运行授权不一致"));
    fixture.store.set_project_status(&fixture.project, ProjectStatus::Paused)?;
    assert!(read(&worktree, "allowed.txt").is_err());
    Ok(())
}

#[test]
fn research_gateway_reads_actual_random_content_rejects_write_and_applies_live_tightening() {
    let fixture = Fixture::new();
    let marker = format!("PRIVATE_PROBE_{}", id());
    std::fs::write(fixture.root.join("project/probe.txt"), &marker).unwrap();
    let (turn, run) = active_turn(&fixture);
    let call = |name, args:Value| research::call(&fixture.store, &fixture.project, &turn.task_id, &run, &fixture.agents[0], name, &args);
    assert_eq!(call("cc_research_read", json!({"path":"probe.txt"})).unwrap()["text"], marker);
    assert!(call("cc_research_write", json!({"path":"probe.txt","text":"changed"})).is_err());
    assert!(call("cc_message_send", json!({"body":"unauthorized"})).is_err());
    assert_eq!(std::fs::read_to_string(fixture.root.join("project/probe.txt")).unwrap(), marker);
    fixture.store.conversation_policy_set(ConversationPolicySetInput { project_id:fixture.project.clone(), agent_id:None, boundary:ConversationHardBoundary { network:"none".into(), denied_read_paths:vec!["probe.txt".into()], ..Default::default() } }).unwrap();
    assert!(call("cc_research_read", json!({"path":"probe.txt"})).unwrap_err().to_string().contains("长期限制"));
    assert!(call("cc_research_web_search", json!({"query":"OpenAI"})).is_err());
    assert!(research::call(&fixture.store, &fixture.project, &turn.task_id, "different-run", &fixture.agents[0], "cc_research_list", &json!({"path":"."})).is_err());
    let events = fixture.store.runtime_events(&fixture.project, None, 2000).unwrap();
    assert!(events.iter().any(|e| e.event_type == "tool_result" && e.payload["status"] == "completed"));
    assert!(events.iter().any(|e| e.event_type == "permission_denied"));
}

#[test]
fn relaxing_settings_cannot_widen_a_persisted_request() {
    let fixture = Fixture::new();
    fixture.store.conversation_policy_set(ConversationPolicySetInput { project_id:fixture.project.clone(), agent_id:None, boundary:ConversationHardBoundary { tools:"none".into(), ..Default::default() } }).unwrap();
    let (turn, _) = active_turn(&fixture);
    fixture.store.conversation_policy_set(ConversationPolicySetInput { project_id:fixture.project.clone(), agent_id:None, boundary:Default::default() }).unwrap();
    assert!(!fixture.store.conversation_task_policy(&turn.task_id, &fixture.agents[0]).unwrap().allows_tools());
}

#[test]
fn research_paths_reject_traversal_prefix_collision_secrets_ads_and_junctions() {
    let fixture = Fixture::new(); let root = fixture.root.join("project");
    std::fs::create_dir_all(root.join("private")).unwrap();
    std::fs::create_dir_all(fixture.root.join("project-other")).unwrap();
    std::fs::write(root.join(".env"), "DO_NOT_READ").unwrap();
    std::fs::write(root.join("public.txt"), "public").unwrap();
    std::fs::write(root.join("private/secret.txt"), "private").unwrap();
    let boundary = ConversationHardBoundary { denied_read_paths:vec!["private".into()], ..Default::default() };
    for path in ["../project-other", ".env", ".env:secret", "public.txt:stream", "private/secret.txt"] {
        assert!(research::readable_path(&root, &boundary, path).is_err(), "must refuse {path}");
    }
    assert!(research::readable_path(&root, &boundary, &fixture.root.join("project-other").display().to_string()).is_err());
    #[cfg(windows)] {
        let link = root.join("linked"); junction::create(fixture.root.join("project-other"), &link).unwrap();
        assert!(research::readable_path(&root, &boundary, "linked").is_err());
        junction::delete(&link).unwrap();
    }
    #[cfg(unix)] {
        std::os::unix::fs::symlink(fixture.root.join("project-other"), root.join("linked")).unwrap();
        assert!(research::readable_path(&root, &boundary, "linked").is_err());
    }
    assert!(research::readable_path(&root, &boundary, "public.txt").is_ok());
}

#[test]
#[ignore = "requires CREWVELA_RUN_REAL_RESEARCH=1; installed logged-in Claude ACP 0.63.0/Pi 1.5.0; four model requests via production executor"]
fn real_claude_pi_research_uses_production_executor_and_preserves_reply_only() {
    assert_eq!(std::env::var("CREWVELA_RUN_REAL_RESEARCH").as_deref(), Ok("1"));
    let base = std::env::var_os("CREWVELA_RESEARCH_PROBE_DIR").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let mut outcomes = Vec::new();
    for runtime in [AgentRuntimeKind::ClaudeCode, AgentRuntimeKind::Pi] {
        let fixture = Fixture::at(base.join(format!("research-{}-{}", runtime.as_str(), id())));
        let marker = format!("RESEARCH_{}", id());
        std::fs::write(fixture.root.join("project/probe.txt"), &marker).unwrap();
        fixture.store.connection.lock().unwrap().execute("UPDATE ma_agents SET runtime_kind=?2 WHERE id=?1", params![fixture.agents[0], runtime.as_str()]).unwrap();
        let sessions = crate::sessions::SessionManager::open_at(fixture.root.join("sessions.json"), fixture.root.join("events")).unwrap();
        let executor = ExistingRunExecutor { host:AgentTerminalHost::new(crate::pty::PtyManager::new(), sessions), bridge:CollaborationBridge::start(fixture.store.clone()).unwrap() };
        let mut evidence = json!({"runtime":runtime,"root":fixture.root,"replyOnly":false,"read":false,"webFetch":false,"webSearch":false});
        let result = (|| -> Result<()> {
            let mut input = fixture.input(); input.participant_agent_ids = vec![fixture.agents[0].clone()]; input.max_duration_seconds = 180;
            input.body = "本次只回复 REPLY_ONLY_OK，不调用任何工具。".into();
            let (round, _) = fixture.store.begin_conversation(input.clone())?;
            run_discussion(&fixture.store, &round.id, &Arc::new(AtomicBool::new(false)), &executor)?;
            let snapshot = fixture.store.conversation_snapshot(&fixture.project)?;
            if snapshot.rounds[0].status != "completed" { bail!("reply-only failed: {:?}", snapshot.rounds[0].stop_reason); }
            if !snapshot.messages.iter().any(|m| m.sender_type == "agent" && m.body.contains("REPLY_ONLY_OK")) { bail!("reply-only marker missing"); }
            let prior_events = fixture.store.runtime_events(&fixture.project, None, 2000)?;
            if prior_events.iter().any(|e| matches!(e.event_type.as_str(), "tool_start" | "tool_protocol")) { bail!("reply-only unexpectedly exposed tools"); }
            evidence["replyOnly"] = json!(true);
            input.client_message_id = "research-next-message".into();
            input.body = "请用cc_research_read读取当前项目中的probe.txt，准确返回文件中的随机字符串（不能猜测）；随后用cc_research_web_fetch读取https://example.com并用cc_research_web_search搜索OpenAI Codex documentation，报告每个工具的实际结果、来源URL和查询时间。某项失败时继续其他项并如实报告。不得修改文件、发信或调用其他工具。".into();
            let (round, _) = fixture.store.begin_conversation(input)?;
            run_discussion(&fixture.store, &round.id, &Arc::new(AtomicBool::new(false)), &executor)?;
            let snapshot = fixture.store.conversation_snapshot(&fixture.project)?;
            let events = fixture.store.runtime_events(&fixture.project, None, 2000)?;
            evidence["events"] = serde_json::to_value(&events)?;
            evidence["messages"] = serde_json::to_value(&snapshot.messages)?;
            let completed = |name| events.iter().any(|event| event.event_type == "tool_result" && event.payload["toolName"] == name && event.payload["status"] == "completed");
            evidence["read"] = json!(completed("cc_research_read") && snapshot.messages.iter().any(|m| m.sender_type == "agent" && m.body.contains(&marker)));
            let tool_result = |name:&str| events.iter().rev().find(|event| event.event_type == "tool_result" && event.payload["toolName"] == name && event.payload["status"] == "completed").map(|event| &event.payload["result"]);
            evidence["webFetch"] = json!(tool_result("cc_research_web_fetch").is_some_and(|result| result["text"].as_str().is_some_and(|text| text.contains("Example Domain")) && result["retrievedAt"].is_string()));
            evidence["webSearch"] = json!(tool_result("cc_research_web_search").is_some_and(|result| result["text"].as_str().is_some_and(|text| text.contains("<rss ") && text.contains("<channel>") && text.contains("<item>")) && result["retrievedAt"].is_string()));
            if std::fs::read_to_string(fixture.root.join("project/probe.txt"))? != marker { bail!("project file changed"); }
            if snapshot.rounds.last().unwrap().status != "completed" { bail!("research round failed: {:?}", snapshot.rounds.last().unwrap().stop_reason); }
            if evidence["read"] != true { bail!("no successful real file read with exact random marker"); }
            if evidence["webFetch"] != true || evidence["webSearch"] != true { bail!("public web fetch/search did not both succeed; inspect actual failure evidence"); }
            Ok(())
        })();
        // Preserve errors even if a runtime fails before its first tool.
        evidence["snapshot"] = serde_json::to_value(fixture.store.conversation_snapshot(&fixture.project).unwrap()).unwrap();
        evidence["runtimeEvents"] = serde_json::to_value(fixture.store.runtime_events(&fixture.project, None, 2000).unwrap()).unwrap();
        evidence["error"] = json!(result.as_ref().err().map(|e| format!("{e:#}")));
        let path = fixture.root.join("evidence.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&evidence).unwrap()).unwrap();
        eprintln!("REAL_RESEARCH_EVIDENCE={} replyOnly={} read={} webFetch={} webSearch={} error={}", path.display(), evidence["replyOnly"], evidence["read"], evidence["webFetch"], evidence["webSearch"], evidence["error"]);
        outcomes.push(result.is_ok());
    }
    assert!(outcomes.iter().all(|ok| *ok), "some real runtime checks failed; actual per-runtime evidence retained");
}
