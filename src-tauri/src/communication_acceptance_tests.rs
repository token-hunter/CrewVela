//! Explicit, model-backed acceptance of the actual interactive CLI transport.
//!
//! Run only with `CREWVELA_ACCEPTANCE_EXE` pointing to the current application
//! build and `cargo test --lib real_claude_codex_bidirectional_pty -- --ignored
//! --nocapture`. The normal test suite never launches these CLIs or a model.
//! This tests real receivers and assistant replies; it does not claim that a
//! model invoked `send` (the separate MCP acceptance covers that workflow).

use super::*;
use anyhow::{bail, Context};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

const ACCEPTANCE_TIMEOUT: Duration = Duration::from_secs(240);
const POLL_INTERVAL: Duration = Duration::from_millis(80);

struct AcceptanceHarness {
    root: PathBuf,
    sessions: SessionManager,
    queue: SessionActionQueue,
    pty: PtyManager,
    owned_ptys: Vec<u32>,
    observed_events: HashSet<PathBuf>,
    hooks: Vec<serde_json::Value>,
    deadline: Instant,
    watchdog_cancel: Option<mpsc::Sender<()>>,
    watchdog: Option<std::thread::JoinHandle<()>>,
}

impl AcceptanceHarness {
    fn new() -> anyhow::Result<Self> {
        let root = std::env::temp_dir().join(format!(
            "crewvela-real-cli-acceptance-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir(&root)?;
        Self::open(root)
    }

    fn open(root: PathBuf) -> anyhow::Result<Self> {
        let temp = std::env::temp_dir().canonicalize()?;
        anyhow::ensure!(
            root.canonicalize()?.starts_with(&temp),
            "acceptance root escaped the temporary directory"
        );
        let sessions = SessionManager::open_at(root.join("sessions.json"), root.join("events"))?;
        let queue = SessionActionQueue::open_at(root.join("mailbox.json"), root.join("outbox"))?;
        let pty = PtyManager::new();
        let (cancel, stopped) = mpsc::channel();
        let watchdog_pty = pty.clone();
        let watchdog_root = root.clone();
        let watchdog = std::thread::spawn(move || {
            if matches!(
                stopped.recv_timeout(ACCEPTANCE_TIMEOUT),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                let _ = std::fs::write(
                    watchdog_root.join("watchdog.txt"),
                    "Total acceptance deadline exceeded; stopping only this harness's PTYs.\n",
                );
                let _ = watchdog_pty.kill_all();
            }
        });
        eprintln!("REAL_CLI_ACCEPTANCE_ROOT={}", root.display());
        Ok(Self {
            root,
            sessions,
            queue,
            pty,
            owned_ptys: Vec::new(),
            observed_events: HashSet::new(),
            hooks: Vec::new(),
            deadline: Instant::now() + ACCEPTANCE_TIMEOUT,
            watchdog_cancel: Some(cancel),
            watchdog: Some(watchdog),
        })
    }

    fn start(
        &mut self,
        kind: AgentRuntimeKind,
        executable: &Path,
    ) -> anyhow::Result<SessionRecord> {
        let runtime = kind.as_str();
        let cwd = self.root.join(runtime);
        std::fs::create_dir(&cwd)?;
        anyhow::ensure!(
            std::fs::read_dir(&cwd)?.next().is_none(),
            "CLI workspace must initially be empty"
        );
        anyhow::ensure!(
            cwd.canonicalize()?.starts_with(self.root.canonicalize()?),
            "refusing to trust a non-fixture workspace"
        );
        let record = self.sessions.create_agent(sessions::NewAgentSession {
            avatar_id: None,
            project_id: "isolated-communication-acceptance".into(),
            agent_id: runtime.into(),
            runtime_kind: runtime.into(),
            agent_name: format!("Acceptance {runtime}"),
            cwd: cwd.display().to_string(),
        })?;
        let path = multi_agent::resolve_runtime_executable(kind)
            .with_context(|| format!("installed {runtime} CLI was not found"))?;
        let mut command = multi_agent::command_for_path(&path);
        // Match canonical project paths stored by the multi-agent panel on Windows.
        command.current_dir(cwd.canonicalize()?).env("TERM", "xterm-256color");
        command.env_remove("CLAUDECODE");
        let no_tools = "This is an isolated communication acceptance test. Do not use any tools, skills, commands, files, subagents, MCP servers, or network tools. The incoming message contains a unique marker. Reply with exactly that marker and nothing else. Do not execute the reply command shown in the message wrapper.";
        if kind == AgentRuntimeKind::ClaudeCode {
            let test_exe = std::env::current_exe()?.display().to_string();
            let hook = self
                .sessions
                .hook_command(&record.id)?
                .replace(&test_exe, &executable.display().to_string());
            anyhow::ensure!(
                hook.contains(&executable.display().to_string()) && !hook.contains(&test_exe),
                "Claude hook must invoke the application executable"
            );
            let mut identity = HashMap::new();
            // Preserve only the user's configured provider environment, never
            // their hooks. Credentials are neither logged nor saved in fixtures.
            let settings_path = dirs::home_dir().context("home directory unavailable")?.join(".claude/settings.json");
            if settings_path.exists() {
                let configured: serde_json::Value = serde_json::from_slice(&std::fs::read(settings_path)?)?;
                if let Some(env) = configured["env"].as_object() {
                    for (key, value) in env {
                        if let Some(value) = value.as_str() { identity.insert(key.clone(), value.to_string()); }
                    }
                }
            }
            identity.extend(HashMap::from([
                ("CREWVELA_SESSION_ID".to_string(), record.id.clone()),
                ("CREWVELA_CLI".to_string(), executable.display().to_string()),
                ("DISABLE_AUTOUPDATER".to_string(), "1".to_string()),
            ]));
            let settings = pty::build_settings_json(&identity, &hook);
            // Explicit settings contain the isolated hook only. User/project
            // hooks must not accidentally write to the running application's
            // production events directory.
            command.args(["--setting-sources", "", "--settings", &settings]);
            command.args(["--session-id", &uuid::Uuid::new_v4().to_string()]);
            command.args([
                "--tools",
                "",
                "--strict-mcp-config",
                "--mcp-config",
                "{\"mcpServers\":{}}",
            ]);
            command.args([
                "--disable-slash-commands",
                "--no-chrome",
                "--append-system-prompt",
                no_tools,
            ]);
        } else {
            let mut notify = self.sessions.agent_turn_hook_command(&record.id)?;
            notify[0] = executable.display().to_string();
            command.args([
                "--no-alt-screen",
                "--no-daemon",
                "--sandbox",
                "read-only",
                "--ask-for-approval",
                "on-request",
            ]);
            command
                .arg("-c")
                .arg(format!("notify={}", serde_json::to_string(&notify)?));
            command.arg("-c").arg(format!(
                "developer_instructions={}",
                serde_json::to_string(no_tools)?
            ));
            // These feature switches remove the shell execution tool. Codex
            // currently has no public CLI equivalent of Claude's --tools "";
            // the no-tool instruction and exact reply are checked separately.
            command.args(["--disable", "shell_tool", "--disable", "unified_exec"]);
            command.arg("-c").arg("web_search=\"disabled\"");
            // Keep the installed toolchain fixed throughout acceptance. This
            // override applies only to this child, not the user's config file.
            command.arg("-c").arg("check_for_update_on_startup=false");
            // Trust only this freshly-created empty fixture, per invocation.
            // This grants neither hook trust nor approval/sandbox bypass.
            command.arg("-c").arg(format!(
                "projects.{}.trust_level=\"trusted\"",
                serde_json::to_string(&cwd.display().to_string())?
            ));
            for (key, value) in [
                ("CREWVELA_SESSION_ID", record.id.clone()),
                ("CREWVELA_CLI", executable.display().to_string()),
            ] {
                command.env(key, &value);
                command.arg("-c").arg(format!(
                    "shell_environment_policy.set.{key}={}",
                    serde_json::to_string(&value)?
                ));
            }
        }
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        anyhow::ensure!(
            !args
                .iter()
                .any(|arg| arg.contains("bypass") || arg.contains("dangerously")),
            "acceptance must preserve ordinary permissions"
        );
        // Automatic workspace confirmation is scoped by the verified empty cwd
        // above. No permission, login, hook-trust, or update prompt is approved.
        let pty_id = self.pty.open_agent_program(&command, &record.id, true)?;
        self.owned_ptys.push(pty_id);
        self.pty.resize(pty_id, 40, if kind == AgentRuntimeKind::Codex { 60 } else { 120 })?;
        let started = self.sessions.attach_runtime(&record.id, pty_id)?;
        self.write_json(&format!("{runtime}-launch.json"), &serde_json::json!({
            "appSessionId": started.id, "runtimeKind": runtime,
            "cliExecutable": path, "hookExecutable": executable,
            "cwd": cwd, "ptyId": pty_id, "permissionBypass": false,
            "isolation": "temporary app sessions, events, mailbox and outbox; installed CLI accounts",
            "toolBoundary": if kind == AgentRuntimeKind::ClaudeCode { "built-in tools disabled; strict empty MCP config" } else { "shell features disabled; no-tool instruction; exact completion required; not a universal capability denylist" },
        }))?;
        Ok(started)
    }

    fn write_json(&self, name: &str, value: &serde_json::Value) -> anyhow::Result<()> {
        std::fs::write(self.root.join(name), serde_json::to_vec_pretty(value)?)?;
        Ok(())
    }

    fn tick(&mut self) -> anyhow::Result<()> {
        // Capture only non-sensitive completion/lifecycle fields, apply the
        // production hook state machine, then remove precisely that source.
        // A second directory-wide ingest here could consume a newly-arrived
        // completion between our scan and its scan without recording evidence.
        // Input echoes and PTY rendering never count as assistant completion.
        let mut paths = std::fs::read_dir(self.root.join("events"))?
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            if !self.observed_events.insert(path.clone()) {
                continue;
            }
            let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&path)?)?;
            self.hooks.push(serde_json::json!({
                "event": value["hook_event_name"],
                "appSessionId": value["_crewvela_app_session_id"],
                "runtimeSessionId": value["session_id"],
                "lastAssistantMessage": value["last_assistant_message"],
                "notificationType": value["notification_type"],
                "queuedAt": value["_crewvela_queued_at"],
            }));
            self.sessions.apply_hook_value(&value)?;
            std::fs::remove_file(&path)?;
        }
        let chunks = self.pty.poll_all();
        for session in self.sessions.list().iter().filter(|session| session.agent_runtime_kind.as_deref() == Some("codex")) {
                if let Some(pty_id) = session.pty_id {
                    let bytes = self.pty.transcript_tail(pty_id, 32_768)?;
                    let text = String::from_utf8_lossy(&bytes).to_lowercase();
                    if text.contains("update available") && text.contains("esc skip") {
                        bail!("Codex displayed an update menu despite per-process update checks being disabled; no input will be sent");
                    }
                }
        }
        mark_interactive_runtime_prompts(&chunks, &self.sessions);
        // Keep frame boundaries for the isolated no-tool fixture: this catches
        // partial TUI redraws that a flattened transcript cannot reproduce.
        for (id, chunk) in &chunks {
            if !chunk.data.is_empty() {
                use std::io::Write;
                let lowered = chunk.data.to_ascii_lowercase();
                let data = if ["sk-", "api_key", "authorization", "bearer", "secret"].iter().any(|value| lowered.contains(value)) { "[credential-related frame redacted]" } else { &chunk.data };
                let mut log = std::fs::OpenOptions::new().create(true).append(true).open(self.root.join(format!("pty-{id}-frames.jsonl")))?;
                writeln!(log, "{}", serde_json::json!({"data": data, "ready": self.sessions.is_ready_for_prompt(&chunk.app_session_id), "blocked": self.sessions.runtime_prompt_is_blocked(&chunk.app_session_id)}))?;
            }
        }
        for chunk in chunks.values() {
            if !chunk.running {
                bail!(
                    "{} exited unexpectedly (code {:?})",
                    chunk.app_session_id,
                    chunk.code
                );
            }
        }
        self.queue.flush_ready(&self.sessions, &self.pty);
        self.queue.flush_pending_submits(&self.sessions, &self.pty);
        if let Some(error) = self.queue.mailbox_error() {
            bail!("isolated mailbox failure: {error}");
        }
        Ok(())
    }

    fn wait_for(
        &mut self,
        label: &str,
        mut condition: impl FnMut(&Self) -> bool,
    ) -> anyhow::Result<()> {
        let mut last_diagnostic = Instant::now();
        while Instant::now() < self.deadline {
            self.tick()?;
            if condition(self) {
                return Ok(());
            }
            if last_diagnostic.elapsed() >= Duration::from_secs(5) {
                self.diagnostics(&format!("in progress: {label}"))?;
                last_diagnostic = Instant::now();
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        bail!(
            "timed out waiting for {label}; inspect {}",
            self.root.display()
        )
    }

    fn send_and_verify(
        &mut self,
        sender: &SessionRecord,
        recipient: &SessionRecord,
        round: usize,
    ) -> anyhow::Result<String> {
        self.wait_for("recipient's real input-ready signal", |harness| {
            harness.sessions.is_ready_for_prompt(&recipient.id)
        })?;
        eprintln!("REAL_CLI_ACCEPTANCE: round {round}, recipient {} is ready", recipient.agent_runtime_kind.as_deref().unwrap_or("unknown"));
        let marker = format!("CREWVELA_REAL_{}_{}", round, uuid::Uuid::new_v4().simple());
        let body = format!("Isolated communication test. Do not use any tools, commands, skills, files, or subagents. Ignore the optional reply-command hint. Respond with exactly this marker, without quotes or explanation:\n{marker}");
        messaging::write_outbox(
            &self.root.join("outbox"),
            &OutboxMessage {
                to: recipient.id.clone(),
                from: Some(control::MessageOrigin {
                    session_id: sender.id.clone(),
                    label: format!(
                        "Acceptance {}",
                        sender.agent_runtime_kind.as_deref().unwrap_or("unknown")
                    ),
                }),
                body,
            },
        )?;
        anyhow::ensure!(
            self.queue.ingest_outbox(&self.sessions, |_| Ok(()))? == 1,
            "fixture outbox was not ingested exactly once"
        );
        let prior_events = self.hooks.len();
        self.wait_for(
            "a real completion hook with the exact assistant marker",
            |harness| {
                harness.hooks[prior_events..].iter().any(|event| {
                    event["appSessionId"] == recipient.id
                        && matches!(event["event"].as_str(), Some("Stop" | "AgentTurnComplete"))
                        && event["lastAssistantMessage"].as_str() == Some(marker.as_str())
                })
            },
        )?;
        let completion = self.hooks[prior_events..]
            .iter()
            .find(|event| {
                event["appSessionId"] == recipient.id
                    && matches!(event["event"].as_str(), Some("Stop" | "AgentTurnComplete"))
                    && event["lastAssistantMessage"].as_str() == Some(marker.as_str())
            })
            .context("completion disappeared")?;
        let runtime_id = completion["runtimeSessionId"]
            .as_str()
            .filter(|id| !id.is_empty())
            .context("completion omitted native runtime session ID")?
            .to_string();
        let current = self
            .sessions
            .get(&recipient.id)
            .context("original app session disappeared")?;
        anyhow::ensure!(
            current.id == recipient.id && current.pty_id == recipient.pty_id,
            "delivery changed the recipient's app session or process"
        );
        anyhow::ensure!(
            current.claude_session_id == runtime_id,
            "hook was not bound to the recipient's actual native session"
        );
        let delivery = self
            .queue
            .delivery_records()
            .into_iter()
            .find(|record| record.to_session_id == recipient.id && record.body.contains(&marker))
            .context("no queue transport evidence for completed marker")?;
        anyhow::ensure!(
            delivery.stage == "submit_sent" && delivery.error.is_none(),
            "message did not complete the queue submit stage"
        );
        anyhow::ensure!(
            delivery.from_session_id.as_deref() == Some(sender.id.as_str()),
            "sender identity changed"
        );
        self.write_json(
            &format!("round-{round}.json"),
            &serde_json::json!({
                "marker": marker, "fromAppSessionId": sender.id, "toAppSessionId": recipient.id,
                "runtimeSessionId": runtime_id, "completion": completion,
                "delivery": delivery, "evidence": "real CLI completion hook, not PTY input echo",
                "scope": "CLI receive and model reply; model did not invoke send",
            }),
        )?;
        eprintln!("REAL_CLI_ACCEPTANCE: round {round} verified exact model reply and native session {runtime_id}");
        Ok(runtime_id)
    }

    fn diagnostics(&self, outcome: &str) -> anyhow::Result<()> {
        self.write_json(
            "summary.json",
            &serde_json::json!({
                "outcome": outcome, "sessions": self.sessions.list(), "hooks": self.hooks,
                "readiness": self.sessions.list().iter().map(|session| serde_json::json!({
                    "appSessionId": session.id,
                    "ready": self.sessions.is_ready_for_prompt(&session.id),
                    "blocked": self.sessions.runtime_prompt_is_blocked(&session.id),
                })).collect::<Vec<_>>(),
                "deliveries": self.queue.communication_evidence(),
                "productionAppDataTouched": false,
            }),
        )?;
        // Rendered text is diagnostic only. Strip control sequences and redact
        // secret-like tokens; never use its marker echo as a success assertion.
        for pty_id in &self.owned_ptys {
            if let Ok(bytes) = self.pty.transcript_tail(*pty_id, 48 * 1024) {
                let text = String::from_utf8_lossy(&bytes);
                let redacted = redact_diagnostic(&text);
                std::fs::write(
                    self.root.join(format!("pty-{pty_id}-diagnostic.txt")),
                    redacted,
                )?;
            }
        }
        Ok(())
    }
}

impl Drop for AcceptanceHarness {
    fn drop(&mut self) {
        if std::thread::panicking() {
            let _ = self.diagnostics("panicked");
        } else if !self.root.join("summary.json").exists() {
            let _ = self.diagnostics("harness stopped; see result.json");
        }
        for pty_id in &self.owned_ptys {
            if let Err(error) = self.pty.kill(*pty_id) {
                let _ = std::fs::write(
                    self.root.join(format!("pty-{pty_id}-cleanup-error.txt")),
                    format!("{error:#}"),
                );
            }
        }
        // open_agent_program can retain a tracked process when pipe setup
        // fails before returning its numeric ID. This manager belongs solely
        // to the fixture, so include those partially-started owned processes.
        if let Err(error) = self.pty.kill_all() {
            let _ = std::fs::write(self.root.join("cleanup-error.txt"), format!("{error:#}"));
        }
        if let Some(cancel) = self.watchdog_cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(watchdog) = self.watchdog.take() {
            let _ = watchdog.join();
        }
        // Retain the dedicated root on success and failure for review. No user
        // project, app mailbox, account, or unrelated process is cleaned up.
    }
}

fn redact_diagnostic(raw: &str) -> String {
    // Strip CSI and OSC terminal escapes, including OSC strings terminated by
    // BEL or ESC-backslash. Keep only printable text and line/tab separators.
    let mut plain = String::new();
    let mut escape = 0;
    for character in raw.chars() {
        match (escape, character) {
            (0, '\x1b') => escape = 1,
            (0, ch) if !ch.is_control() || matches!(ch, '\n' | '\t') => plain.push(ch),
            (0, _) => {}
            (1, '[') => escape = 2,
            (1, ']') => escape = 3,
            (1, _) => escape = 0,
            (2, ch) if ('@'..='~').contains(&ch) => escape = 0,
            (2, _) => {}
            (3, '\x07') | (4, '\\') => escape = 0,
            (3, '\x1b') => escape = 4,
            (3, _) => {}
            (4, _) => escape = 3,
            _ => escape = 0,
        }
    }
    plain
        .lines()
        .map(|line| {
            let lowered = line.to_ascii_lowercase();
            if [
                "sk-",
                "api_key",
                "apikey",
                "api-key",
                "authorization",
                "bearer",
                "secret",
                "token",
            ]
            .iter()
            .any(|needle| lowered.contains(needle))
            {
                "[credential-related diagnostic line redacted]\n".to_string()
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}

#[test]
#[ignore = "opt-in: requires installed authenticated Claude Code and Codex CLIs and real model calls; CREWVELA_ACCEPTANCE_EXE must point to the current application build"]
fn real_claude_codex_bidirectional_pty() -> anyhow::Result<()> {
    let executable = PathBuf::from(std::env::var_os("CREWVELA_ACCEPTANCE_EXE").context(
        "set CREWVELA_ACCEPTANCE_EXE to the current crewvela executable before opting in",
    )?);
    let executable = std::path::absolute(executable)?;
    anyhow::ensure!(
        executable.is_file(),
        "CREWVELA_ACCEPTANCE_EXE must be a file"
    );
    anyhow::ensure!(
        executable.canonicalize()? != std::env::current_exe()?.canonicalize()?,
        "the Rust test binary cannot serve application hook subcommands"
    );
    let mut harness = AcceptanceHarness::new()?;
    let result = (|| -> anyhow::Result<()> {
        let claude = harness.start(AgentRuntimeKind::ClaudeCode, &executable)?;
        let codex = harness.start(AgentRuntimeKind::Codex, &executable)?;
        let codex_native = harness.send_and_verify(&claude, &codex, 1)?;
        let claude_native = harness.send_and_verify(&codex, &claude, 2)?;
        // Let post-completion redraws settle before a late incoming message.
        let idle_until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < idle_until {
            harness.tick()?;
            std::thread::sleep(POLL_INTERVAL);
        }
        // A second round proves the actual native session IDs remain stable,
        // rather than accepting a newly-created history with the same title.
        anyhow::ensure!(
            harness.send_and_verify(&claude, &codex, 3)? == codex_native,
            "Codex switched native sessions between messages"
        );
        anyhow::ensure!(
            harness.send_and_verify(&codex, &claude, 4)? == claude_native,
            "Claude switched native sessions between messages"
        );
        harness.write_json("result.json", &serde_json::json!({
            "passed": true, "realModelReplies": 4, "directions": ["claude_code -> codex", "codex -> claude_code"],
            "stableNativeSessions": true, "permissionBypass": false,
            "evidence": "exact last_assistant_message from Stop/AgentTurnComplete hooks",
            "modelInvokedSend": false,
        }))?;
        Ok(())
    })();
    if let Err(error) = &result {
        let _ = harness.write_json("result.json", &serde_json::json!({ "passed": false, "error": format!("{error:#}"), "diagnostics": harness.root }));
    }
    harness.diagnostics(if result.is_ok() { "passed" } else { "failed" })?;
    result
}

#[test]
#[ignore = "requires the installed Codex CLI and Node; runs a loopback model with an isolated Codex home"]
fn installed_codex_handoff_submits_the_complete_prompt_once() -> anyhow::Result<()> {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Stdio};

    const CHILD: &str = "CREWVELA_HANDOFF_PTY_TEST_ROOT";
    struct StopChild(Child);
    impl Drop for StopChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let Some(root) = std::env::var_os(CHILD).map(PathBuf::from) else {
        // AF_UNIX control sockets on Windows have a 108-byte path limit.
        let id = uuid::Uuid::new_v4().simple().to_string();
        let root = std::env::temp_dir().join(format!("cvh-{}", &id[..8]));
        let home = root.join("c");
        std::fs::create_dir(&root)?;
        let output = std::process::Command::new(std::env::current_exe()?)
            .args(["installed_codex_handoff_submits_the_complete_prompt_once", "--ignored", "--nocapture", "--test-threads=1"])
            .env(CHILD, &root)
            .env("CODEX_HOME", &home)
            .env("CODEX_SQLITE_HOME", &home)
            .env_remove("OPENAI_API_KEY").env_remove("CODEX_API_KEY")
            .output()?;
        if home.join("app-server-daemon/daemon.pid").is_file() {
            let executable = multi_agent::resolve_runtime_executable(AgentRuntimeKind::Codex)
                .context("Codex CLI unavailable for isolated daemon cleanup")?;
            let mut stop = multi_agent::command_for_path(&executable);
            stop.args(["app-server", "daemon", "stop"])
                .env("CODEX_HOME", &home)
                .env("CODEX_SQLITE_HOME", &home)
                .env_remove("OPENAI_API_KEY")
                .env_remove("CODEX_API_KEY");
            let _ = title_summary::run_cli_with_timeout(
                stop, None, Duration::from_secs(15), 64 * 1024, "stop fixture Codex daemon",
            );
        }
        anyhow::ensure!(output.status.success() && String::from_utf8_lossy(&output.stdout).contains("1 passed"),
            "installed Codex handoff failed:\n{}\n{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
        std::fs::remove_dir_all(root)?;
        return Ok(());
    };
    anyhow::ensure!(root.canonicalize()?.starts_with(std::env::temp_dir().canonicalize()?));
    let home = root.join("c");
    anyhow::ensure!(std::env::var_os("CODEX_HOME").as_deref() == Some(home.as_os_str()));
    crate::test_profile::initialize(&root.join("crewvela"))?;
    let cwd = root.join("workspace");
    std::fs::create_dir(&home)?;
    std::fs::create_dir(&cwd)?;
    let proof = root.join("received.jsonl");
    let server = r#"
const http = require('node:http'), fs = require('node:fs');
http.createServer(async (req, res) => {
  if (req.method !== 'POST') { res.writeHead(404); res.end(); return; }
  const chunks = [];
  for await (const chunk of req) chunks.push(chunk);
  const payload = JSON.parse(Buffer.concat(chunks));
  const prompts = (payload.input || []).filter(item => item.role === 'user')
    .flatMap(item => item.content || []).map(item => item.text || '');
  fs.appendFileSync(process.argv[1], JSON.stringify(prompts) + '\n');
  const item = { id:'msg_handoff', type:'message', role:'assistant', status:'completed',
    content:[{type:'output_text', text:'HANDOFF_NATIVE_RECEIPT_OK', annotations:[]}] };
  res.writeHead(200, {'Content-Type':'text/event-stream'});
  const write = (type, fields) => res.write('event: ' + type + '\ndata: ' + JSON.stringify({type, ...fields}) + '\n\n');
  write('response.created', {response:{id:'resp_handoff', created_at:1, status:'in_progress', output:[]}});
  write('response.output_item.added', {output_index:0, item:{...item, content:[], status:'in_progress'}});
  write('response.output_item.done', {output_index:0, item});
  write('response.completed', {response:{id:'resp_handoff', created_at:1, status:'completed', output:[item],
    usage:{input_tokens:1, output_tokens:1, total_tokens:2}}});
  res.end();
}).listen(0, '127.0.0.1', function() { console.log('http://127.0.0.1:' + this.address().port + '/v1'); });
"#;
    let mut provider = StopChild(std::process::Command::new(which::which("node")?)
        .args(["-e", server]).arg(&proof).stdout(Stdio::piped()).spawn()?);
    let mut endpoint = String::new();
    BufReader::new(provider.0.stdout.take().context("missing loopback endpoint")?).read_line(&mut endpoint)?;
    let endpoint = endpoint.trim();
    anyhow::ensure!(endpoint.starts_with("http://127.0.0.1:"));
    std::fs::write(home.join("config.toml"), format!(
        "model = 'gpt-5.4'\nmodel_provider = 'handoff'\ncheck_for_update_on_startup = false\n[model_providers.handoff]\nname = 'Local test'\nbase_url = '{endpoint}'\nwire_api = 'responses'\nrequest_max_retries = 0\nstream_max_retries = 0\n"
    ))?;

    let mut harness = AcceptanceHarness::open(crate::providers::config_dir()?)?;
    harness.deadline = Instant::now() + Duration::from_secs(45);
    let source = harness.sessions.create_runtime(sessions::NewRuntimeSession {
        runtime_kind: "claude_code".into(), cwd: cwd.display().to_string(), project_id: None,
    })?;
    let target = harness.sessions.create_runtime(sessions::NewRuntimeSession {
        runtime_kind: "codex".into(), cwd: cwd.display().to_string(), project_id: None,
    })?;
    let handoff = uuid::Uuid::new_v4().to_string();
    let body = format!(
        "[CrewVela handoff {handoff}]\n{}\n  保留 \"引号\"、C:\\demo\\file、%PATH% 和 &\n最后一行 →°×🐍",
        "君子和而不同。测试交接正文保留换行、符号和中英文。abcdefghijklmnopqrstuvwxyz\n".repeat(22),
    );
    let mut message = control::QueuedMessage::new(body.clone(), None);
    message.id = handoff;
    session_switch::seed_codex_handoff_for_test(&source, &target, &message, None)?;
    harness.queue.enqueue_codex_handoff(&target.id, message.clone(), None)?;
    let running = open_runtime_session_for_switch(&target, &harness.pty, &harness.sessions, &message.id)
        .map_err(anyhow::Error::msg)?;
    let pty = running.pty_id.context("Codex handoff did not open a PTY")?;
    harness.owned_ptys.push(pty);
    harness.pty.resize(pty, 34, 78)?;
    harness.wait_for("the exact Codex handoff to be durably accepted", |h| {
        proof.is_file() && h.queue.peek_next(&target.id).is_none()
    })?;
    harness.wait_for("the Codex reply in the original target terminal", |h| {
        h.pty.transcript_tail(pty, 64 * 1024).is_ok_and(|bytes| {
            String::from_utf8_lossy(&bytes).contains("HANDOFF_NATIVE_RECEIPT_OK")
        })
    })?;
    let requests = std::fs::read_to_string(proof)?;
    let user_inputs: Vec<String> = requests
        .lines()
        .map(serde_json::from_str::<Vec<String>>)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .flatten()
        .collect();
    let received: Vec<_> = user_inputs
        .iter()
        .filter(|prompt| prompt.starts_with("[CrewVela handoff "))
        .collect();
    anyhow::ensure!(
        received.len() == 1 && *received[0] == body,
        "Codex did not receive exactly one untruncated handoff ({} handoff prompts)",
        received.len(),
    );
    Ok(())
}
