//! Explicit live-model acceptance for both installed ZCode distributions.
use crate::multi_agent::zcode::{launch, runtime_executable, DesktopConnection};
use crate::multi_agent::*;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::{Duration, Instant};

fn permission(params: &Value, _: &AtomicBool) -> Result<bool> {
    let name = params["toolName"]
        .as_str()
        .or_else(|| params["tool"]["name"].as_str())
        .unwrap_or("");
    Ok(name.contains("crewvela")
        && (name.ends_with("cc_message_send") || name.ends_with("cc_thread_read")))
}

fn prompt(
    connection: &mut DesktopConnection<'_>,
    body: &str,
    stopped: &AtomicBool,
) -> Result<Vec<Value>> {
    let command = connection.send_text(body, stopped, &mut permission)?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        connection.poll(stopped, &mut permission)?;
        if connection.take_auth_error().is_some() {
            bail!("ZCode model authentication failed");
        }
        match connection.turn_state(&command) {
            Some("completedSuccess") => {
                let rows = connection.rows();
                let turn = rows
                    .iter()
                    .find(|row| row["kind"] == "turnHeader" && row["sourceCommandId"] == command)
                    .and_then(|row| row["turnId"].as_str())
                    .context("completed turn has no stable turn id")?;
                return Ok(rows
                    .iter()
                    .filter(|row| row["turnId"] == turn && row["kind"] != "reasoning")
                    .cloned()
                    .collect());
            }
            Some("failed" | "completedInterrupted") => {
                bail!("ZCode model turn did not finish successfully")
            }
            _ => {}
        }
        if Instant::now() >= deadline {
            connection.cancel(stopped, &mut permission)?;
            bail!("ZCode model turn timed out");
        }
    }
}

fn completed_thread_read(rows: &[Value]) -> bool {
    rows.iter().any(|row| {
        row["kind"] == "toolCall"
            && row["status"] == "success"
            && (row["toolName"]
                .as_str()
                .is_some_and(|name| name.contains("crewvela") && name.ends_with("cc_thread_read"))
                || (row["display"]["serverName"] == "crewvela"
                    && row["display"]["toolName"] == "cc_thread_read"))
    })
}

#[test]
#[ignore = "opt-in real ZCode Desktop and CLI models, uses isolated storage and six model prompts"]
fn real_zcode_desktop_cli_message_roundtrip() -> Result<()> {
    anyhow::ensure!(
        std::env::var("CREWVELA_RUN_REAL_COLLABORATION").as_deref() == Ok("1"),
        "explicit real-model opt-in required"
    );
    let root = std::env::temp_dir().join(format!(
        "crewvela-zcode-communication-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root)?;
    eprintln!("ZCODE_COMMUNICATION_ROOT={}", root.display());
    std::fs::write(
        root.join("result.json"),
        br#"{"passed":false,"stage":"starting_native_connections"}"#,
    )?;
    let store = MultiAgentStore::open_at(root.join("control.db"))?;
    let project = store.create_project(CreateProjectInput {
        name: "ZCode communication acceptance".into(),
        goal: "exchange random markers".into(),
        root_path: root.display().to_string(),
        default_branch: "main".into(),
    })?;
    store.set_project_status(&project.id, ProjectStatus::Active)?;
    let bridge = CollaborationBridge::start(store.clone())?;
    let mut peers = Vec::new();
    for kind in [AgentRuntimeKind::ZCode, AgentRuntimeKind::ZCodeCli] {
        let cwd = root.join(kind.as_str());
        std::fs::create_dir(&cwd)?;
        let agent = store.create_agent(CreateAgentInput {
            project_id: project.id.clone(),
            name: kind.as_str().into(),
            runtime_kind: kind,
            role: AgentRole::Worker,
            description: "isolated live acceptance".into(),
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
            title: kind.as_str().into(),
            description: String::new(),
            acceptance_criteria: "exact scoped messages".into(),
            verification_command: None,
            assignee_agent_id: Some(agent.id.clone()),
            reviewer_agent_id: None,
            workspace_mode: WorkspaceMode::SharedWrite,
            priority: 0,
            required_capabilities: vec![],
            max_attempts: 1,
            retry_backoff_seconds: 0,
        })?;
        let run = store.claim_task(ClaimTaskInput {
            task_id: task.id.clone(),
            agent_id: agent.id.clone(),
            lease_seconds: 600,
        })?;
        peers.push((kind, agent, task, run, cwd));
    }
    let leases = peers
        .iter()
        .map(|(_, a, t, r, cwd)| bridge.register_run(&project.id, &t.id, &r.id, &a.id, cwd))
        .collect::<Vec<_>>();
    let stopped = AtomicBool::new(false);
    let pid_a = AtomicU32::new(0);
    let pid_b = AtomicU32::new(0);
    let mut progress_a = || Ok(());
    let mut progress_b = || Ok(());
    let make_command = |index: usize| -> Result<std::process::Command> {
        let mut command = launch(&runtime_executable(peers[index].0)?)?;
        command.env("ZCODE_STORAGE_DIR", peers[index].4.join("storage"));
        Ok(command)
    };
    let mcp = |index: usize| {
        let access = leases[index].access();
        vec![
            json!({"name":"crewvela", "type":"http", "url":format!("{}/mcp",access.endpoint),
            "headers":[{"name":"Authorization","value":format!("Bearer {}",access.token)}], "isolation":"session"}),
        ]
    };
    let mut connections = vec![
        DesktopConnection::open_tracked(
            make_command(0)?,
            &peers[0].4,
            None,
            true,
            mcp(0),
            &stopped,
            &mut permission,
            &mut progress_a,
            Some(&pid_a),
        )?,
        DesktopConnection::open_tracked(
            make_command(1)?,
            &peers[1].4,
            None,
            true,
            mcp(1),
            &stopped,
            &mut permission,
            &mut progress_b,
            Some(&pid_b),
        )?,
    ];
    let native_ids = connections
        .iter()
        .map(|connection| connection.session_id.clone())
        .collect::<Vec<_>>();
    let result = (|| -> Result<Value> {
        for (index, connection) in connections.iter_mut().enumerate() {
            let configured = std::env::var(if index == 0 {
                "CREWVELA_REAL_ZCODE_MODEL"
            } else {
                "CREWVELA_REAL_ZCODE_CLI_MODEL"
            })
            .ok();
            let selection = configured
                .or_else(|| {
                    connection
                        .models
                        .iter()
                        .find(|model| {
                            model.selection["providerId"]
                                == "account:bigmodel-individual-coding-plan"
                        })
                        .map(|model| model.id.clone())
                })
                .or_else(|| connection.selected_model_id.clone())
                .context("no configured ZCode model available")?;
            connection.set_model(&selection, &stopped, &mut permission)?;
            std::fs::write(
                root.join(format!("model-{}.json", peers[index].0.as_str())),
                serde_json::to_vec_pretty(&json!({
                    "runtime": peers[index].0, "selectedModel": selection, "nativeSession": native_ids[index],
                    "models": connection.models.iter().map(|model| &model.id).collect::<Vec<_>>(),
                }))?,
            )?;
        }
        let markers = [
            format!("ZCODE_REAL_{}", uuid::Uuid::new_v4().simple()),
            format!("ZCODE_REAL_{}", uuid::Uuid::new_v4().simple()),
        ];
        for index in 0..2 {
            let peer = 1 - index;
            prompt(&mut connections[index], &format!(
                "Isolated communication acceptance. Use only crewvela MCP tool cc_message_send. Send body exactly {} to targetAgentId {}. Do not use other tools or write files. After the tool succeeds reply SENT.", markers[index], peers[peer].1.id
            ), &stopped)?;
            let snapshot = store.snapshot(&project.id)?;
            let message = snapshot
                .messages
                .iter()
                .find(|message| message.body == markers[index])
                .context("model did not persist expected message")?;
            anyhow::ensure!(
                message.sender_id.as_deref() == Some(peers[index].1.id.as_str())
                    && message.target_agent_id.as_deref() == Some(peers[peer].1.id.as_str())
                    && message.task_id.as_deref() == Some(peers[index].2.id.as_str()),
                "message identity mismatch"
            );
            eprintln!(
                "ZCODE_COMMUNICATION: {} sent exact marker",
                peers[index].0.as_str()
            );
        }
        for index in 0..2 {
            let peer = 1 - index;
            let rows = prompt(&mut connections[index], &format!(
                "Use only crewvela MCP cc_thread_read and cc_message_send. Read your received thread, find the received ZCODE_REAL_ message from {}, then send body consisting of ACK: immediately followed by its exact body back to targetAgentId {}. Insert NO space or newline between the colon and ZCODE_REAL_. Do not guess the received value. Do not use any other tools. Reply with the received body after sending.", peers[peer].1.id, peers[peer].1.id
            ), &stopped)?;
            std::fs::write(
                root.join(format!("rows-{}-ack.json", peers[index].0.as_str())),
                serde_json::to_vec_pretty(&rows)?,
            )?;
            let expected = format!("ACK:{}", markers[peer]);
            anyhow::ensure!(
                completed_thread_read(&rows),
                "ACK turn did not complete a real thread-read tool call"
            );
            let snapshot = store.snapshot(&project.id)?;
            let message = snapshot
                .messages
                .iter()
                .find(|message| message.body == expected)
                .context("model did not persist exact acknowledgement")?;
            anyhow::ensure!(
                message.sender_id.as_deref() == Some(peers[index].1.id.as_str())
                    && message.target_agent_id.as_deref() == Some(peers[peer].1.id.as_str())
                    && message.task_id.as_deref() == Some(peers[index].2.id.as_str()),
                "acknowledgement identity mismatch"
            );
            eprintln!(
                "ZCODE_COMMUNICATION: {} read peer marker and replied",
                peers[index].0.as_str()
            );
        }
        for index in 0..2 {
            let before = store.snapshot(&project.id)?.messages.len();
            let rows = prompt(&mut connections[index], "Use only crewvela MCP tool cc_thread_read. Read the newest received ACK:ZCODE_REAL_ acknowledgement and reply with its exact body. Do not invent it or send new messages. No other tools.", &stopped)?;
            std::fs::write(
                root.join(format!("rows-{}-receipt.json", peers[index].0.as_str())),
                serde_json::to_vec_pretty(&rows)?,
            )?;
            let expected = format!("ACK:{}", markers[index]);
            anyhow::ensure!(
                completed_thread_read(&rows),
                "receipt turn did not complete a real thread-read tool call"
            );
            anyhow::ensure!(
                rows.iter().any(|row| row["kind"] == "assistantText"
                    && row["text"]
                        .as_str()
                        .is_some_and(|text| text.contains(&expected))),
                "sender did not read its exact acknowledgement"
            );
            anyhow::ensure!(
                store.snapshot(&project.id)?.messages.len() == before,
                "receipt read created a reply loop"
            );
            anyhow::ensure!(
                connections[index].session_id == native_ids[index],
                "native session changed"
            );
        }
        let snapshot = store.snapshot(&project.id)?;
        anyhow::ensure!(
            snapshot
                .messages
                .iter()
                .filter(|message| message.sender_type == "agent")
                .count()
                == 4,
            "unexpected extra agent messages"
        );
        Ok(
            json!({"passed":true,"modelPrompts":6,"agentMessages":4,"nativeSessionIds":native_ids,"stableNativeSessions":true,"markers":markers}),
        )
    })();
    let mut cleanup_errors = Vec::new();
    for connection in &mut connections {
        if let Err(error) = connection.shutdown() {
            cleanup_errors.push(format!("{error:#}"));
        }
    }
    drop(connections);
    if pid_a.load(Ordering::Acquire) != 0 || pid_b.load(Ordering::Acquire) != 0 {
        cleanup_errors.push("ZCode acceptance child did not stop".into());
    }
    let evidence = match &result {
        _ if !cleanup_errors.is_empty() => json!({"passed":false,"cleanupErrors":cleanup_errors,
            "modelError":result.as_ref().err().map(|error|format!("{error:#}"))}),
        Ok(value) => value.clone(),
        Err(error) => json!({"passed":false,"error":format!("{error:#}")}),
    };
    std::fs::write(
        root.join("result.json"),
        serde_json::to_vec_pretty(&evidence)?,
    )?;
    anyhow::ensure!(
        cleanup_errors.is_empty(),
        "ZCode cleanup failed: {}",
        cleanup_errors.join("; ")
    );
    result?;
    Ok(())
}
